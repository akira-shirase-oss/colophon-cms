<script>
	import Status from '#lib/components/Status.svelte';
</script>

Publishing an approved changeset makes a **release**, and every release has a **generation**: a number that goes up by exactly one per release in an environment, and never goes down. Two people publishing at once can't both win, and a reader never sees half of a release.

## One transaction, from the base you saw

`POST …/changesets/{id}/publish` carries the changeset's hash in `If-Match`, an `Idempotency-Key`, and `{"base_generation": N}`: the latest release the publisher had seen. In one database transaction:

1. `UPDATE environments SET generation = generation + 1 WHERE generation = N`. If no row matches, another release committed first: `409`, `details.reason` `generation_moved`, with the current generation in `Colophon-Generation`. Nothing is written.
2. The `releases` row is inserted, with the review that approved it.
3. For each pinned revision not already published, the entry's open publication is closed at the new generation, and a copy of the revision is inserted as published from that generation on.
4. The worker's jobs for the release go in the same transaction ([the outbox](/concepts/outbox-and-worker/)).

The publisher's base matters: someone who looked at generation 4 can't publish over a generation 5 they never saw. The conditional update makes "two releases on one base can't both commit" a fact the database enforces, not a check that can race.

## Publication history as intervals

Each entry's published history is a series of half-open intervals of generations, `[from, to)`. An exclusion constraint over `int8range(valid_from, valid_to)` per entry makes overlapping intervals impossible: at any generation, an entry has at most one published revision.

## Reading a release, now or as it was

The public API, `/content/v1/{project}/{environment}/{collection}[/{entry}]`, needs no sign-in. Without parameters it reads the latest generation; with `?generation=N` it reads that release exactly as it was. Every answer names its generation in `Colophon-Generation`, and a list's cursor carries the generation of its first page, so paging through a list never mixes two releases.

Delivery reads a copy of what was published, in a transaction that runs as `colophon_delivery`, a database role granted only reads of published rows and environments' generations. A bug in delivery can't leak a draft: the database refuses. [Delivery API](/reference/delivery-api/) has the details.

## Generations only move forward

This is invariant 7. A slow, stale worker can never put an older release back on the site: deliveries carry their generation, and the worker never sends an older one after a newer one ([the outbox](/concepts/outbox-and-worker/)). When rollback arrives, it will be a new release whose content is older, so the numbers still go up. <Status planned="P4" />

Each environment also has an **epoch**, which is already part of cache keys and webhook payloads. A restore from backup will start a new epoch, so generations from before and after a restore are never confused. <Status planned="P6" />

## How it is tested

- `releases.rs` races two releases on one base, in the database and end to end: exactly one commits.
- The [simulator](/concepts/simulator/)'s publishing scenario checks after every step that generations are contiguous, history never overlaps and every acknowledged publish is a release.
- The real-process crash loop kills the server mid-publish, resends unanswered requests with the same keys, and checks that each project's generations run 1 to N with each changeset published once, and that delivery at generation g serves what was published by then.

## Design

[Module 01, domain](/design/modules/01-domain/), [module 02, db](/design/modules/02-db/) and [module 06, delivery](/design/modules/06-delivery/); decisions D-045 and D-047 in the [decisions](/design/decisions/). On GitHub: [docs/modules/06-delivery.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/06-delivery.md).
