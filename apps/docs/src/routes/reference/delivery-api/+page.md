<script>
	import Status from '#lib/components/Status.svelte';
</script>

`/content/v1` is how a website reads what a project published. It needs no sign-in, it only ever sees published content, and it can read any past release exactly as it was. It runs inside the same server as the management API, on the same port. Handler: `crates/colophon-api/src/delivery.rs`; cache: `crates/colophon-api/src/cache.rs` and `crates/colophon-domain/src/cache.rs`. The ideas are in [Caching and delivery](/concepts/caching-and-delivery/), [Releases and generations](/concepts/releases-and-generations/) and [Unpublish and revocation](/concepts/unpublish-and-revocation/).

## Addresses

`{project}` is the project's id, `{environment}` its slug (`production`), `{collection}` a content type's slug and `{entry}` the entry's id. Projects are addressed by id: a public lookup by slug isn't built, and neither is reading an entry by its URL name.

Every route answers anyone, and none sends CORS headers: read it from your site's server or build step, or through a proxy on your site's own origin, not from browser code on another origin.

## Routes

### `GET /content/v1/{project}/{environment}/{collection}`

The collection's published entries, newest first.

| Query | Meaning |
|---|---|
| `generation` | Read the release with this generation, as it was. Without it, the latest. A generation that hasn't committed is `422` `invalid_generation`. |
| `limit` | Entries per page, 1 to 100, default 20. |
| `cursor` | The `next_cursor` of the previous page. It carries the generation of the first page, so paging through a list never mixes two releases, even if a new one is published meanwhile; a cursor page is a pinned read. A cursor from another list is refused. |

Unknown query parameters are ignored.

```json
{
  "data": [
    {
      "id": "0193f6b0-2c4d-7e5f-8a91-b2c3d4e5f607",
      "revision_id": "0193f6b4-1e2f-7a3b-8c4d-5e6f7a8b9c0d",
      "collection": "pages",
      "title": "Release notes",
      "slug": "release-notes",
      "content": { "title": "Release notes", "slug": "release-notes", "body": { "schema": "colophon/rich-text", "version": 1, "doc": { "type": "doc", "content": [ { "type": "paragraph", "content": [ { "type": "text", "text": "Hello" } ] } ] } } },
      "content_hash": "sha256:9f2c4e1a7b3d5f6e8a0c2b4d6f8e1a3c5b7d9f0e2a4c6b8d0f1e3a5c7b9d2f4e",
      "published_in": 41
    }
  ],
  "next_cursor": null
}
```

A collection with nothing published at that generation, or that doesn't exist, is an empty page, not a `404`. Before the environment's first release, a list is an empty page at generation 0.

### `GET /content/v1/{project}/{environment}/{collection}/{entry}`

One published entry.

| Query | Meaning |
|---|---|
| `generation` | As for the list. |

```json
{
  "data": {
    "id": "0193f6b0-2c4d-7e5f-8a91-b2c3d4e5f607",
    "revision_id": "0193f6b4-1e2f-7a3b-8c4d-5e6f7a8b9c0d",
    "collection": "articles",
    "title": "Release notes",
    "content": { "title": "Release notes", "body": { "schema": "colophon/rich-text", "version": 1, "doc": { "type": "doc", "content": [ { "type": "paragraph", "content": [ { "type": "text", "text": "Hello" } ] } ] } } },
    "content_hash": "sha256:9f2c4e1a7b3d5f6e8a0c2b4d6f8e1a3c5b7d9f0e2a4c6b8d0f1e3a5c7b9d2f4e",
    "published_in": 41
  }
}
```

## The answer

| Field | Meaning |
|---|---|
| `id`, `revision_id` | The entry, and the exact revision readers get. |
| `collection`, `title` | Its collection, and its title in that revision. |
| `slug` <Status building="3.6" /> | Its URL name, when its content type has a slug field and the entry sets it. Left out otherwise, so answers cached before slugs existed read the same. |
| `content` | The revision's canonical JSON, exactly as stored. |
| `content_hash` | SHA-256 of `content`. |
| `published_in` | The generation of the release that published this revision. |

Rich text arrives as `colophon/rich-text` JSON, never HTML: render it with the shared renderer (`packages/content-renderer`) or your own, following the [format](/reference/rich-text/).

## Headers

| Header | Meaning |
|---|---|
| `Colophon-Generation` | The release this answer was read at. |
| `ETag` | Strong: the first half of the body's SHA-256, as 32 hex digits in quotes. The generation isn't part of it, so an answer a later release didn't change still revalidates as unchanged. |
| `Cache-Control` | `public, max-age=0, must-revalidate, s-maxage=30` for the latest release; `public, max-age=0, must-revalidate, s-maxage=300` for a pinned read (`generation` given) or a cursor page. Browsers always revalidate; shared caches may keep it that long. |
| `Cache-Tag` | Comma-separated tags for purging by tag (Cloudflare's header name), below. |
| `Colophon-Cache` | How the server's own cache answered: `hit`, `miss` (read from PostgreSQL by this request, then cached), `coalesced` (waited for another request's read of the same key) or `bypass` (the cache was off, down or slow, or the answer isn't cached). |

Send an `ETag` back in `If-None-Match` and an unchanged answer is `304 Not Modified`, with the same headers and no body. The comparison is weak, as RFC 9110 asks: `W/` tags, lists and `*` work.

```http
GET /content/v1/0193f6a2-5b1c-7d3e-9f40-1a2b3c4d5e6f/production/articles/0193f6b0-2c4d-7e5f-8a91-b2c3d4e5f607 HTTP/1.1
If-None-Match: "3f1c9a7e0b2d4f6a8c1e3b5d7f9a0c2e"

HTTP/1.1 304 Not Modified
Colophon-Generation: 42
Cache-Control: public, max-age=0, must-revalidate, s-maxage=30
ETag: "3f1c9a7e0b2d4f6a8c1e3b5d7f9a0c2e"
Colophon-Cache: hit
Cache-Tag: colophon:p:0193f6a25b1c7d3e9f401a2b3c4d5e6f,colophon:p:0193f6a25b1c7d3e9f401a2b3c4d5e6f:e:0193f6a25b1d7a008b11223344556677,colophon:p:0193f6a25b1c7d3e9f401a2b3c4d5e6f:e:0193f6a25b1d7a008b11223344556677:c:articles,colophon:p:0193f6a25b1c7d3e9f401a2b3c4d5e6f:e:0193f6a25b1d7a008b11223344556677:n:0193f6b02c4d7e5f8a91b2c3d4e5f607
```

### Cache tags

Tags never depend on the generation, so one purge reaches pinned and latest copies alike. Ids are written as 32 hex digits without hyphens.

| Tag | On |
|---|---|
| `colophon:p:<project>` | every answer |
| `colophon:p:<project>:e:<environment id>` | every answer |
| `colophon:p:<project>:e:<environment id>:c:<collection>` | every answer from a collection whose slug is made only of lowercase letters, digits and underscores |
| `colophon:p:<project>:e:<environment id>:n:<entry>` | one entry's answer |

A `cdn` [website connection](/reference/api/website-connections/) is sent the entry and collection tags of everything that changed ([Webhooks](/reference/webhooks/#the-purge-request)). Content-type slugs may contain hyphens, and today a collection such as `release-notes` gets no collection tag, so a purge reaches its entries' answers but not its list pages, which stay in a CDN until their `s-maxage` runs out (30 seconds for the latest).

## The server's cache

With `COLOPHON_CACHE_URL` set (Valkey, or anything that speaks the Redis protocol), answers are cached under keys that name everything they depend on: the project, the environment, its recovery epoch, the generation, the environment's latest takedown, a render version and the normalised query.

- **The current generation always comes from PostgreSQL**, in one statement on every request, never from the cache, so no cache can serve an older release as the latest. A generation's content never changes, so an entry under its key can't go stale; a new release means new keys.
- **A takedown retires old answers**: keys name the environment's latest release that took something down, so answers cached before it are never served again, at any generation.
- Entries live 300 seconds plus up to 60 of jitter. Answers over 1 MiB, empty pages, `404`s and `410`s are served, never cached.
- Misses for one key in one process share one read of PostgreSQL (`coalesced`).
- **When the cache is down or slow**, reads go straight to PostgreSQL and say `bypass`. A cache call that takes over 100 ms is abandoned; after a failure, the server leaves the cache alone for a second before trying to reconnect, so a dead cache costs one attempt a second, not one per request. `/readyz` reports `degraded`, still serving. PostgreSQL stays bounded either way: reads go through its connection pool, and a full pool answers `503` with `Retry-After: 1`.

## Answers that aren't content

| Status | `code` | When |
|---|---|---|
| `400` | `bad_request` | A `limit` outside 1 to 100, a cursor that isn't from this list, or a `generation` that isn't a whole number. |
| `404` | `not_found` | No such project or environment, or (reading one entry) no published entry with that id at that generation, including before the first release. |
| `410` | `gone` | The entry was published and has been taken down, at or before the generation asked for. `Cache-Control: no-store`. |
| `422` | `validation_failed` | `details.reason` `invalid_generation`: a release that hasn't committed. |
| `503` | `unavailable` | No database connection came free in time, with `Retry-After: 1`. If only the cache is down, reads still work. |

Errors carry no `Colophon-Generation`, `ETag` or `Cache-Tag`.

## What it guarantees

- **Only published content.** Each read runs as `colophon_delivery`, a database role that can't read drafts at all.
- **No mixed releases** within one answer, or across the pages of one list.
- **The current release comes from PostgreSQL** on every request, so no cache can serve an older release as the latest (F06, F08; `crates/colophon-server/tests/cache.rs`).
- **Taken down means taken down** at the origin, at every generation (F07; `crates/colophon-server/tests/purges.rs`).

The load scenario is `perf/k6/delivery.js`; [Performance](/resources/performance/) has the measured results, with the machine and date.

## Not built yet

- Reading an entry by its URL name: today a site lists the collection and maps each `slug` to its `id`. <Status planned="not scheduled" />
- Filters, sorting, field selection and reference population beyond paging. <Status planned="D" />
- A TypeScript SDK with generated types. <Status planned="D" />
- Sitemaps, feeds, redirects (including for a changed URL name) and preview links. <Status planned="P4" />
- Locales and locale fallback. <Status planned="E" />
- Private projects whose published content needs a token. <Status planned="not scheduled" />
