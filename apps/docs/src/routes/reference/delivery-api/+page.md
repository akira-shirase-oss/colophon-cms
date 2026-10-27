<script>
	import Status from '#lib/components/Status.svelte';
</script>

`/content/v1` is how a website reads what a project published. It needs no sign-in, it only ever sees published content, and it can read any past release exactly as it was. It runs inside the same server as the management API, on the same port.

## Routes

```http
GET /content/v1/{project}/{environment}/{collection}?generation=&limit=&cursor=
GET /content/v1/{project}/{environment}/{collection}/{entry}?generation=
```

`{project}` is the project's id, `{environment}` its slug (`production`), `{collection}` the content type's slug and `{entry}` the entry's id. Projects are addressed by id: a public lookup by slug isn't built.

| Parameter | Meaning |
|---|---|
| `generation` | Read the release with this generation, as it was. Without it, the latest. A generation that hasn't committed is `422` with `details.reason` `invalid_generation`. |
| `limit` | Entries per page, 1 to 100 (default 20). |
| `cursor` | The `next_cursor` of the previous page. It carries the generation of the first page, so paging through a list never mixes two releases, even if a new one is published meanwhile. A cursor from another list is refused. |

## An answer

```json
{
  "data": {
    "id": "0192c1…",
    "revision_id": "0192d4…",
    "collection": "articles",
    "title": "Release notes",
    "content": { "title": "Release notes", "body": { "schema": "colophon/rich-text", "version": 1, "doc": { … } } },
    "content_hash": "sha256:9f2c…",
    "published_in": 41
  }
}
```

`content` is the revision's canonical JSON, exactly as stored. `published_in` is the generation of the release that published this revision. A list is `{"data": [...], "next_cursor": …}`. Rich text arrives as `colophon/rich-text` JSON, never HTML: render it with the shared renderer (`packages/content-renderer`) or your own, following the [format](/reference/rich-text/).

## Headers

| Header | Meaning |
|---|---|
| `Colophon-Generation` | The release this answer was read at. |
| `ETag` | Strong: the first half of the body's SHA-256. Send it back in `If-None-Match` and an unchanged answer is `304`. |
| `Cache-Control` | `public, max-age=0, must-revalidate`, plus `s-maxage=30` for the latest release or `s-maxage=300` for a pinned one or a cursor page. Browsers always revalidate; shared caches may keep it that long. |
| `Cache-Tag` | Tags for purging: the project, environment, entry and collection. |
| `Colophon-Cache` | How the server's own cache answered: `hit`, `miss`, `coalesced` (shared another request's fill) or `bypass` (the cache was unavailable). |

## Answers that aren't content

| Status | When |
|---|---|
| `404` | No such project or environment, or (reading one entry) no published entry with that id. A list with nothing published is an empty page, not a 404. |
| `410` | The entry was published and has been taken down, at or before the generation asked for. `Cache-Control: no-store`. |
| `400` | A `limit` outside 1 to 100, or a cursor that isn't from this list. |
| `422` | `invalid_generation`: a release that hasn't committed. |
| `503` | The database is unavailable, with `Retry-After`. If only the cache is down, reads still work, straight from PostgreSQL. |

## What it guarantees

- **Only published content.** Each read runs as `colophon_delivery`, a database role that can't read drafts at all.
- **No mixed releases** within one answer, or across the pages of one list.
- **The current release comes from PostgreSQL** on every request, so no cache can serve an older release as the latest ([the delivery cache](/concepts/delivery-cache/)).
- **Taken down means taken down** at the origin, at every generation ([unpublish and revocation](/concepts/unpublish-and-revocation/)).

## Not built yet

Locales and locale fallback <Status planned="later" />; filters, sorting and field selection beyond paging <Status planned="not scheduled" />; private projects whose published content needs a token <Status planned="not scheduled" />; sitemaps, feeds, redirects and preview links <Status planned="P4" />; a generated TypeScript client <Status planned="not scheduled" />.
