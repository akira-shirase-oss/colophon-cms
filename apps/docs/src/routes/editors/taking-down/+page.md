Taking something down removes an article from the website. It changes what readers see, so it goes through the same steps as publishing: a publish request, a review, and a release with its own number.

## Taking an article down

1. Make a publish request (**Changesets**, then **New changeset**: the Studio's older word, until step 3.7) and tick **Unpublish** beside the article. That pins the version readers see now.
2. Submit it for review. The reviewer sees exactly what goes away; the article is marked *unpublish*.
3. Once it is approved, publish it.

The release says what it did: *1 unpublished* on **Releases**, *1 taken down* on its receipt. The article stays in the Studio with all its versions, and its state goes back to **Draft**: taking it down removes it from the website, not from your history. You can publish it again later; it reappears from that new release on.

## What happens

- From that release on, the public API answers that the article is gone (`410 Gone`), and leaves it out of lists.
- Old addresses that read a past release stop serving it too. Once it is taken down, Colophon's own server never delivers that content again, at any release.
- If the project clears a CDN on each release, the release asks the CDN to drop the article and its collection's lists. Until the CDN accepts, the release says *Removal pending at the CDN: copies it holds may still be served*.
- If the website is rebuilt on each release, a site built from the new release doesn't include the article.

## What can't be taken back

Be clear about the limits before you rely on taking something down for anything sensitive:

- **Copies already out there stay out there.** Readers' browsers, screenshots, feeds that were fetched, archives and anyone who downloaded the page keep what they have. Nothing can recall a download.
- **A CDN accepting the request isn't proof the copies are gone** ([why](/editors/receipts/#why-purge-accepted-isnt-gone)). If clearing the CDN fails, its copies can be served until they expire, up to 5 minutes.
- **It waits for a reviewer.** There is no emergency takedown that skips review, and none is on the roadmap yet.

## Technical details

Taking down is a changeset item of kind `unpublish` (`"unpublish": [ids]` when creating the changeset), pinned to the published revision. Publishing it writes a **tombstone** at the new generation: from then on the delivery API answers `410 gone` for the entry, uncached, at the latest release and at every `?generation=` read, and cache keys change so no answer cached before it is served again. CDN purges are jobs of the `cdn` destinations. [Unpublish and revocation](/concepts/unpublish-and-revocation/) and [Delivery API](/reference/delivery-api/) have the rest.
