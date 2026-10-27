<script>
	import Status from '#lib/components/Status.svelte';
</script>

An entry is one piece of content in a collection. Every save is a new **revision** (a *version* in the Studio) that never changes; the entry's draft points at the latest one. Saves name the revision they build on, so a stale save is refused, never merged. This page covers the entry routes and the rich-text check. Handlers: `crates/colophon-api/src/entries.rs` and `rich_text.rs`. The ideas are in [Drafts and versions](/concepts/drafts-and-versions/) and [Saves and conflicts](/concepts/saves-and-conflicts/); the editor's side is [Writing and saving](/editors/writing/) and [Versions and comparing](/editors/versions/).

`{collection}` is a content type's slug, `articles` in every new project. Content is checked against the collection's [content type](/reference/api/content-types/) on every create and save.

Since R.2 each route checks its permission on this collection: `content.read`, `content.create` or `content.update`, on `*` or on the collection's slug <Status building="R.2" />. Every built-in role holds all three everywhere, so on `main` any member may read and write entries. Since 3.6, creates and saves take the collection's lock shared, so they wait while a [content type change](/reference/api/content-types/#changing-a-content-type) is being checked <Status building="3.6" />.

## The draft object

A create, a save and a read answer with the entry's draft:

```json
{
  "data": {
    "id": "0193f6b0-2c4d-7e5f-8a91-b2c3d4e5f607",
    "collection": "articles",
    "created_at": "2026-10-27T09:10:02.316552Z",
    "updated_at": "2026-10-27T09:14:03.512841Z",
    "revision": {
      "id": "0193f6b4-1e2f-7a3b-8c4d-5e6f7a8b9c0d",
      "base_revision_id": "0193f6b1-7d8e-7f90-a1b2-c3d4e5f60718",
      "author_id": "0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d",
      "created_at": "2026-10-27T09:14:03.512841Z",
      "content_hash": "sha256:9f2c4e1a7b3d5f6e8a0c2b4d6f8e1a3c5b7d9f0e2a4c6b8d0f1e3a5c7b9d2f4e",
      "title": "Release notes"
    },
    "content": {
      "title": "Release notes",
      "body": { "schema": "colophon/rich-text", "version": 1, "doc": { "type": "doc", "content": [ { "type": "paragraph", "content": [ { "type": "text", "text": "Hello" } ] } ] } }
    }
  }
}
```

| Field | Meaning |
|---|---|
| `id` | The entry's id, `{entry}` in paths. |
| `created_at`, `updated_at` | When the entry was created; when its draft last moved. |
| `revision.id` | The draft's revision: the entry's `ETag` is this id in quotes. |
| `revision.base_revision_id` | The revision this one was saved on top of; `null` for an entry's first. |
| `revision.author_id` | Who saved it (an identity id). |
| `revision.content_hash` | SHA-256 of the canonical content. |
| `revision.title` | The value of the content type's title field. |
| `content` | The canonical content exactly as stored: fields in definition order, absent optional fields left out, rich text canonical. A URL name, when the type has one, is a field here like any other. |

## Routes

### `GET {P}/collections/{collection}/entries`

The collection's entries, newest first, each with its draft's revision and what readers get now.

- **Who:** `content.read` (every built-in role).
- **Query:**

| Parameter | Meaning |
|---|---|
| `limit` | Entries per page, 1 to 100, default 20. |
| `cursor` | The `next_cursor` of the previous page. It works only for this project, environment and collection. |
| `q` | Searches draft titles: matches anywhere, ignoring case; `%`, `_` and `\` match themselves. Trimmed; empty means no search; at most 100 characters. |

**Response:** `200`, ordered by when each entry was created (newest first), not by when it was last saved.

```json
{
  "data": [
    {
      "id": "0193f6b0-2c4d-7e5f-8a91-b2c3d4e5f607",
      "updated_at": "2026-10-27T09:14:03.512841Z",
      "revision": { "id": "0193f6b4-1e2f-7a3b-8c4d-5e6f7a8b9c0d", "base_revision_id": "0193f6b1-7d8e-7f90-a1b2-c3d4e5f60718", "author_id": "0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d", "created_at": "2026-10-27T09:14:03.512841Z", "content_hash": "sha256:9f2c…", "title": "Release notes" },
      "published": { "revision_id": "0193f6b1-7d8e-7f90-a1b2-c3d4e5f60718", "generation": 41 }
    }
  ],
  "next_cursor": "djE6MDE5M2Y2YTItNWIxYy03ZDNlLTlmNDAtMWEyYjNjNGQ1ZTZmOnByb2R1Y3Rpb246YXJ0aWNsZXM6MDE5M2Y2YjAtMmM0ZC03ZTVmLThhOTEtYjJjM2Q0ZTVmNjA3"
}
```

`published` is `null` for an entry readers don't have. When its `revision_id` differs from the draft's, the entry has changed since it went live; `generation` is the release it went out in.

| Status | `code` | When |
|---|---|---|
| `400` | `bad_request` | `limit` outside 1 to 100, a cursor from another list, or a search over 100 characters. |
| `403` | `forbidden` | Your role can't read entries here (R.2). |
| `404` | `not_found` | No such project, environment or collection, or you aren't a member. |

### `POST {P}/collections/{collection}/entries`

Creates an entry and its first revision.

- **Who:** `content.create` (every built-in role).
- **Headers:** `Idempotency-Key` (required).
- **Body:** the content itself, as the collection's content type defines it:

```json
{
  "title": "Release notes",
  "body": { "schema": "colophon/rich-text", "version": 1, "doc": { "type": "doc", "content": [ { "type": "paragraph", "content": [ { "type": "text", "text": "Hello" } ] } ] } }
}
```

**Response:** `201 Created`, `ETag: "<revision id>"`, [the draft](#the-draft-object). A replay of the same key and content answers the same, with `Idempotent-Replayed: true`. The answer is sent only after the transaction commits: an acknowledged write is durable.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `400` | `bad_request` | | The `Idempotency-Key` is malformed. |
| `403` | `forbidden` | | Your role can't create entries here. |
| `404` | `not_found` | | No such project, environment or collection. |
| `409` | `conflict` | `idempotency_key_reused` | The key was used for different content. |
| `409` | `conflict` | `slug_taken` | Another entry of the collection has this URL name, as its draft or published; `details.entries` names it <Status building="3.6" />. |
| `422` | `validation_failed` | a [content code](/reference/api/errors/#content-and-content-types) | The content doesn't fit the content type; `details.pointer` points at the value. |
| `428` | `precondition_required` | | No `Idempotency-Key`. |

### `GET {P}/collections/{collection}/entries/{entry}`

The entry's draft.

- **Who:** `content.read`.

**Response:** `200`, `ETag: "<revision id>"`, [the draft](#the-draft-object).

| Status | `code` | When |
|---|---|---|
| `404` | `not_found` | No such entry in this collection. |

### `PUT {P}/collections/{collection}/entries/{entry}`

Saves a new revision on top of the one named in `If-Match`, and moves the draft to it.

- **Who:** `content.update` (every built-in role).
- **Headers:** `If-Match`, the draft's `ETag` as you read it, and `Idempotency-Key` (both required).
- **Body:** the whole content, not a patch, as for creating.

**Response:** `200`, `ETag: "<new revision id>"`, [the draft](#the-draft-object), whose `revision.base_revision_id` is the revision you named.

If the draft has moved on since that revision, nothing is written: the answer is `409` `stale_revision` with the current `ETag`, and the Studio opens its conflict screen ([Saves and conflicts](/concepts/saves-and-conflicts/)).

```http
HTTP/1.1 409 Conflict
ETag: "0193f6b6-0a1b-7c2d-8e3f-4a5b6c7d8e9f"
Content-Type: application/json

{"error":{"code":"conflict","message":"the draft has changed since that revision; load the current one and try again","details":{"reason":"stale_revision","pointer":""},"request_id":"0193f6e2-8b9c-7d0e-9f1a-2b3c4d5e6f70"}}
```

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `400` | `bad_request` | | `If-Match` isn't one revision `ETag` exactly as given (no `*`, no `W/`, no list), or the key is malformed. |
| `403` | `forbidden` | | Your role can't change entries here. |
| `404` | `not_found` | | No such entry in this collection. |
| `409` | `conflict` | `stale_revision` | The draft has moved on; `ETag` names its revision now. |
| `409` | `conflict` | `idempotency_key_reused` | The key was used for a different save. |
| `409` | `conflict` | `slug_taken` | Another entry has this URL name <Status building="3.6" />. |
| `422` | `validation_failed` | a [content code](/reference/api/errors/#content-and-content-types) | The content doesn't fit the content type. |
| `428` | `precondition_required` | | No `If-Match`, or no `Idempotency-Key`. |

Tests: `a_stale_save_is_refused_with_the_current_etag_and_writes_nothing` and `writes_need_their_preconditions_and_keys_replay_or_refuse` in `crates/colophon-server/tests/entries.rs`.

### `GET {P}/collections/{collection}/entries/{entry}/revisions`

The entry's history.

- **Who:** `content.read`.

**Response:** `200`, newest first, the newest 100, without content and without `next_cursor`:

```json
{
  "data": [
    { "id": "0193f6b4-1e2f-7a3b-8c4d-5e6f7a8b9c0d", "base_revision_id": "0193f6b1-7d8e-7f90-a1b2-c3d4e5f60718", "author_id": "0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d", "created_at": "2026-10-27T09:14:03.512841Z", "content_hash": "sha256:9f2c…", "title": "Release notes" },
    { "id": "0193f6b1-7d8e-7f90-a1b2-c3d4e5f60718", "base_revision_id": null, "author_id": "0193f69e-0a1b-7c2d-8e3f-405162738495", "created_at": "2026-10-27T09:10:02.316552Z", "content_hash": "sha256:1b7e…", "title": "Release note" }
  ]
}
```

| Status | `code` | When |
|---|---|---|
| `404` | `not_found` | No such entry in this collection. |

### `GET {P}/collections/{collection}/entries/{entry}/revisions/{revision}`

One revision with its content, for comparing versions.

- **Who:** `content.read`.

**Response:** `200`, no `ETag` (a revision never changes).

```json
{
  "data": {
    "revision": { "id": "0193f6b1-7d8e-7f90-a1b2-c3d4e5f60718", "base_revision_id": null, "author_id": "0193f69e-0a1b-7c2d-8e3f-405162738495", "created_at": "2026-10-27T09:10:02.316552Z", "content_hash": "sha256:1b7e…", "title": "Release note" },
    "content": { "title": "Release note", "body": { "schema": "colophon/rich-text", "version": 1, "doc": { "type": "doc", "content": [ { "type": "paragraph", "content": [ { "type": "text", "text": "Hi" } ] } ] } } }
  }
}
```

| Status | `code` | When |
|---|---|---|
| `404` | `not_found` | No such revision of this entry in this collection. |

### `POST /api/v1/rich-text/validate`

A rich-text document's canonical form, or the rule it breaks, without storing anything. Saves run the same validator; this lets the Studio, the testbed and scripts check a document first.

- **Who:** anyone. It needs no sign-in, no CSRF token and no project.
- **Body:** a [`colophon/rich-text` v1 document](/reference/rich-text/):

```json
{ "schema": "colophon/rich-text", "version": 1, "doc": { "type": "doc", "content": [ { "type": "paragraph", "content": [ { "type": "text", "text": "Hello" } ] } ] } }
```

**Response:** `200`. Unlike the rest of the API, the verdict isn't wrapped in `data`:

```json
{
  "value": { "schema": "colophon/rich-text", "version": 1, "doc": { "type": "doc", "content": [ { "type": "paragraph", "content": [ { "type": "text", "text": "Hello" } ] } ] } },
  "nodes": 3,
  "depth": 3,
  "bytes": 140
}
```

| Field | Meaning |
|---|---|
| `value` | The canonical document: what a save would store. |
| `nodes` | How many nodes, text nodes included (at most 10,000). |
| `depth` | The deepest nesting; `doc` is depth 1 (at most 16). |
| `bytes` | The canonical JSON's size (at most 256 KiB). |

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `422` | `validation_failed` | a [rich-text code](/reference/rich-text/#error-codes), such as `unsafe_link` or `too_deep` | The document breaks the format; `details.pointer` points into it. |

## Not built yet

- Deleting entries: the `content.delete` permission exists, and no route uses it yet. <Status planned="not scheduled" />
- A query API: filters beyond title search, sorting, field selection and reference population. <Status planned="D" />
- Field-level permissions, so a save that changes a field your role may not change is refused with `field_not_permitted`. <Status planned="R.3" />
- Scheduled publishing of an entry. See [Scheduled publishing](/concepts/scheduled-publishing/). <Status building="e/schedules" />
