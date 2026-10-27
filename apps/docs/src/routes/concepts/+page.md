<script>
	import Status from '#lib/components/Status.svelte';
</script>

Each page here explains one idea: what it guarantees, how the code makes it true, which test proves it, and where the guarantee stops. They follow the life of a piece of content, from the first keystroke to a reader's screen. Each page carries its own label: *Done* is merged into `main` with its tests, *Building* is on a branch and not merged, *Planned* is in the roadmap with nothing built.

The editor guide uses plain words for the same things: a changeset is a *publish request*, a revision a *version*, a slug a *URL name*. These pages use the API's words and give the plain one where it helps. The [glossary](/resources/glossary/) has both.

## Writing

- [Drafts and versions](/concepts/drafts-and-versions/): every save is a new version that is never changed; the draft points at the latest. <Status done="1.2" />
- [Saves and conflicts](/concepts/saves-and-conflicts/): why a stale save is refused, and why a retried one is never stored twice. <Status done="1.3" />
- [Content types and URL names](/concepts/content-types/): definitions whose changes are checked against stored content, and URL names no two entries share. <Status building="3.6" />
- [Roles and permissions](/concepts/roles-and-permissions/): one deny-by-default evaluator, roles as data, roles owners define, and nobody handing out more than they hold. <Status building="R.2" />

## Publishing

- [Publish requests](/concepts/publish-requests/): a request pins exact versions, and an approval counts only for those bytes. <Status done="2.1" />
- [Releases and generations](/concepts/releases-and-generations/): one transaction per publish, on the release the publisher saw; numbers that only go up. <Status done="2.2" />
- [The outbox and the worker](/concepts/outbox-and-worker/): telling other systems, without ever holding a transaction open across the network. <Status done="2.3" />
- [Scheduled publishing](/concepts/scheduled-publishing/): publishing at a chosen local time, checked again when it falls due, and labelled delayed when it runs late. <Status building="e/schedules" />

## Delivery and proof

- [Caching and delivery](/concepts/caching-and-delivery/): a cache that can't serve the wrong release, and can be lost without losing anything. <Status done="3.1" />
- [Unpublish and revocation](/concepts/unpublish-and-revocation/): taking content back, and saying plainly what can't be. <Status done="3.2" />
- [Receipts and probes](/concepts/receipts-and-probes/): evidence of what the website shows, scoped to where it was looked at. <Status done="3.3" />

## People who aren't developers

- [Onboarding and plain words](/concepts/onboarding/): one glossary for every Studio screen, one-click publishing and a first-run checklist. <Status building="3.7" />

## Testing it

- [The deterministic simulator](/concepts/simulator/): thousands of seeded runs per push, and a failure you can replay from its number. <Status done="2.4" />

The [security model](/resources/security/) gathers what these pages say about isolation, sign-in and permissions in one place, with what isn't done yet.

## The rules underneath all of them

These come from `AGENTS.md`, and every concept above is one of them applied:

1. **PostgreSQL is the only authority** for content, revisions, releases, permissions and jobs. The cache and the CDN are accelerators that may forget everything.
2. **No database transaction spans an external call.** Commit an intent, do the work, store the evidence, reconcile uncertainty.
3. **States are truthful.** Accepted isn't published, a purge accepted isn't copies gone, and unknown stays unknown.
4. **Everything is bounded**: every queue, body, page, cache, retry loop and pool has a limit and a deadline.
5. **One API, no bypass.** The Studio, the command line and scripts all call the same services with the same checks.
6. **Agents propose; the platform enforces.** The automation this rule governs comes with P5. <Status planned="P5" />
7. **Release generations only move forward.** A rollback is a new generation; a stale worker can never promote an older one.
