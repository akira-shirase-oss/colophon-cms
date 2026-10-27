Each page here explains one idea: what it guarantees, how the code makes it true, how a test proves it, and where the guarantee stops. They follow the life of a piece of content, from the first keystroke to a reader's screen.

## Writing

- [Drafts and revisions](/concepts/drafts-and-revisions/): every save is a new revision that is never changed.
- [Saves, If-Match and conflicts](/concepts/saves-and-conflicts/): why a stale save is refused, and why a retried one is never stored twice.
- [Content types and safe changes](/concepts/content-types/): definitions that can only change in ways that keep stored content valid.
- [Roles and invitations](/concepts/roles/): one policy table, single-use links, and a first administrator made from a terminal.

## Publishing

- [Changesets and hash-bound reviews](/concepts/changesets-and-reviews/): an approval counts only for the exact bytes reviewed.
- [Releases and generations](/concepts/releases-and-generations/): one transaction per publish; numbers that only go up.
- [The outbox and the worker](/concepts/outbox-and-worker/): telling other systems, without ever holding a transaction open across the network.

## Delivery and proof

- [The delivery cache](/concepts/delivery-cache/): a cache that can't serve the wrong release, and can be lost without losing anything.
- [Unpublish and revocation](/concepts/unpublish-and-revocation/): taking content back, and saying honestly what can't be.
- [Delivery receipts and probes](/concepts/receipts-and-probes/): evidence of what the website shows, scoped to where it was looked at.

## Testing it

- [The deterministic simulator](/concepts/simulator/): thousands of seeded runs per push, and a failure you can replay from its number.

## The rules underneath all of them

These come from `AGENTS.md`, and every concept above is one of them applied:

1. **PostgreSQL is the only authority** for content, revisions, releases, permissions and jobs. The cache and the CDN are accelerators that may forget everything.
2. **No database transaction spans an external call.** Commit an intent, do the work, store the evidence, reconcile uncertainty.
3. **States are truthful.** Accepted isn't published, a purge accepted isn't copies gone, and unknown stays unknown.
4. **Everything is bounded**: every queue, body, page, cache, retry loop and pool has a limit and a deadline.
5. **One API, no bypass.** The Studio, the command line and scripts all call the same services with the same checks.
6. **Agents propose; the platform enforces.** <span class="text-muted-foreground">(The automation this governs is planned for P5.)</span>
7. **Release generations only move forward.** A rollback is a new generation; a stale worker can never promote an older one.
