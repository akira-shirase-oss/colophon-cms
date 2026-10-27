<script>
	import Status from '#lib/components/Status.svelte';
</script>

Unpublishing takes an entry off the website. It is a change to what readers see, so it goes through the same steps as publishing: a publish request, a review, and a release with its own number.

## Taking an entry down

1. Make a publish request (**New changeset**) and pick the entry with **Unpublish**. That pins the version readers see now.
2. Submit it for review. The reviewer sees exactly what goes away.
3. Once approved, publish it.

The entry stays in the Studio with all its versions: unpublishing removes it from the website, not from your history. You can publish it again later; it reappears from that new release on.

## What happens

- From that release on, the public API answers **410 Gone** for the entry, and leaves it out of lists.
- Old addresses that read a past release (time-travel reads) stop serving it too. Once taken down, Colophon's own server never delivers that content again, at any release.
- If the project has a CDN connection, the release asks it to purge the entry and its collection's lists. Until the CDN accepts, the release says *Removal pending at the CDN: copies it holds may still be served*.

## What can't be taken back

Be clear about the limits before you rely on unpublishing for something sensitive:

- **Copies already out there stay out there.** Readers' browsers, screenshots, feeds that were fetched, archives and anyone who downloaded the page keep what they have. Nothing can recall a download.
- **A CDN accepting a purge isn't proof the copies are gone** ([why](/editors/website-status/)). If the purge fails, CDN copies can be served until they expire, up to 5 minutes.
- **It waits for a reviewer.** There is no emergency takedown that skips review yet. <Status planned="not scheduled" />
