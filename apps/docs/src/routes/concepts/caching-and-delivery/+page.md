<script>
	import Status from '#lib/components/Status.svelte';
</script>

Public reads are cached, but a cache is where a CMS most easily serves the wrong thing: an old release after a new one, or something that was taken down. Colophon's cache is built so that a stale answer is impossible rather than unlikely, and so that losing the cache loses nothing.

## Keys that name their release

Every `/content/v1` answer is cached in Valkey under a key built by `colophon_domain::cache::Scope` from:

- the render version, project, environment, epoch and **generation**;
- the environment's latest unpublish (so one unpublish retires every answer cached before it);
- the page size, the cursor's position, and the collection name last.

Ids are written as fixed-width hex, so the collection name is the only free text in a key, and parameters the API doesn't know don't change an answer or its key. Because the generation is in the key, a new release doesn't need to invalidate anything: its answers live under new keys.

## The generation always comes from PostgreSQL

Before the cache is asked anything, each request resolves its environment and generation from PostgreSQL in one statement (`delivery_environment`, a function that runs as the read-only delivery role). The cache can hold answers; it can never decide which release is current. So no cache, however stale, can bring an older release back.

That costs one small database read per request. An earlier version did the lookup as a full delivery transaction, five round trips, and the cache saved nothing (D-048 records both measurements).

## One fill per key

On a miss, one request per key per process becomes the leader: it looks once more, reads PostgreSQL at that generation, and stores the answer for 300 seconds plus up to 60 seconds of jitter. Every request waiting on the same key shares its outcome, failures included. A thousand readers arriving at one server process as a hot key expires cause one database read, not a thousand.

Empty pages, `404`s and answers over 1 MiB are served but never stored, so made-up names in URLs can't fill the cache.

## Losing the cache loses nothing

Valkey is configured so it can't keep anything: no snapshots, no append-only file, 128 MiB with least-recently-used eviction. Every Valkey call has a 100 ms deadline. When it fails, the connection is dropped and left alone for a second; one request reconnects at a time and the rest read PostgreSQL, whose pool (`503` with `Retry-After` when full) bounds the load.

## Headers for everyone else's caches

| Header | Value |
|---|---|
| `ETag` | strong: half of a SHA-256 of the body. `If-None-Match` gets `304`. |
| `Cache-Control` | browsers always revalidate (`max-age=0, must-revalidate`); shared caches keep the latest answer up to 30 s (`s-maxage=30`), a pinned one or a cursor page up to 300 s |
| `Cache-Tag` | the project, environment, entry and collection, for purges |
| `Colophon-Cache` | `hit`, `miss`, `coalesced` or `bypass` |
| `Colophon-Generation` | the release the answer was read at |

A shared cache may hold a latest answer up to 30 seconds until a purge reaches it ([Unpublish and revocation](/concepts/unpublish-and-revocation/)). These are policies the tests check, not a promise that every copy everywhere is fresh within 30 seconds.

## How it is tested

F06 (a new matching article reaches cached lists), F08 (cache loss loses nothing) and L04 (a stampede on one key) are in the server's tests, and `perf/k6/delivery.js` is the load scenario. Single-flight is per process: several API processes may each fill a key once.

Not yet: locales in keys (there is one locale) <Status planned="later" />, and a contract test against Redis itself as well as Valkey <Status planned="not scheduled" />.

## Design

[Module 06, delivery](/design/modules/06-delivery/); decisions D-015 and D-048 in the [decisions](/design/decisions/). On GitHub: [docs/modules/06-delivery.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/06-delivery.md).
