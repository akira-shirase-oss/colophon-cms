<script>
	import Status from '#lib/components/Status.svelte';
</script>

Taking content back is harder than publishing it: copies spread. Colophon makes sure its own server never serves an unpublished entry again, at any release, asks CDNs to drop their copies, and says plainly what it can't recall.

## A takedown is a reviewed release

Unpublishing is a changeset item of kind `unpublish` (`POST …/changesets` takes `unpublish: [ids]` beside `entries`). The item pins the revision that is currently published, so the reviewer approves taking down exactly what readers see; it goes stale if another revision is published meanwhile. Publishing it is an ordinary release with its own generation. Agents, when they arrive, can't skip that review either.

## What the release does

In the release's transaction, for each unpublished entry:

1. its open publication is closed, provided it is still the pinned revision (if not, nothing commits);
2. a **tombstone** row records the entry, the generation, its collection and the revision taken down;
3. every earlier publication of the entry is marked `revoked_in` that generation;
4. the environment records the generation as its latest unpublish.

From then on, delivery skips revoked publications in lists, and a read of the entry, at any generation, is `410 Gone` with `Cache-Control: no-store`, never cached. Because cache keys name the latest unpublish, every answer cached before it is retired at once. History rows stay: the record of what was published, and when, isn't rewritten. Publishing the entry again opens a new publication after the tombstone.

## CDN purges

A destination can be a CDN (`kind: "cdn"`). Its job for a release is a purge by tag, covering every release since that CDN's last successful purge, so skipping or superseding one loses nothing:

- each changed entry's tag (every generation's answers for it carry that tag) and its collection's tag (every listing carries it);
- more than 30 tags, and the purge is the environment's tag alone;
- nothing changed, and the job succeeds without a request.

The releases list gives each release a purge state: `none`, `pending`, `accepted` or `failed`. *Accepted* is the CDN's word that it took the request; it isn't an observation that copies are gone. A purge that fails for good leaves copies until they expire (30 s for latest answers, 300 s for pinned ones), and the release says so.

## What can't be recalled

Copies in browsers, readers' downloads, screenshots and anything already fetched are out of reach, and Colophon never claims otherwise. The origin's refusal is what it can guarantee; the CDN's acceptance is reported as acceptance; only a [probe](/concepts/receipts-and-probes/) can say what readers at one place actually get.

## How it is tested

F07 (an entry unpublished while old caches exist: the origin denies it at every generation, the purge is tracked and remaining exposure is visible) runs end to end with a fake CDN (`crates/colophon-server/tests/purges.rs` and `cache.rs`).

Not yet: an emergency takedown that skips review, which would need an online authorization path and no shared cache <Status planned="not scheduled" />; authenticated purges for a real CDN, whose Cloudflare adapter keeps credentials in the environment, never the database <Status planned="P3" />.

## Design

[Module 06, delivery](/design/modules/06-delivery/); decisions D-049 and D-050 in the [decisions](/design/decisions/). On GitHub: [docs/modules/06-delivery.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/06-delivery.md).
