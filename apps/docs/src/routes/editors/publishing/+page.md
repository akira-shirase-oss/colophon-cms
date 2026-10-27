<script>
	import Status from '#lib/components/Status.svelte';
</script>

Publishing turns an approved publish request into a **release**: one numbered step in the website's history, with the time it went live. Everything in the request goes live together, at the same moment, or nothing does. There is no half-published state for a reader to see.

## Publishing an approved request

Owners and publishers see a **Publish** button on an approved request. Press it, and the release is made at once: the public API serves the new versions from that moment, and the request's state becomes *Published*, with a link to *release N*. A request publishes once.

Releases are numbered 1, 2, 3 and so on, per project, and the numbers only go up. **Releases** in the sidebar lists them, newest first, as *No. 1*, *No. 2* and so on: what each changed, who published it, how long ago, and its website label. The foot of the sidebar always shows the release readers have now, under *Live now*, or *Nothing live yet* before the first.

## When someone else published first

Each publish is made on top of the latest release your page had seen. If a colleague published another release in the meantime, yours is refused rather than quietly stacked on something you never looked at. The Studio says so: *Another release was published first. Check what is live now, then publish again.* Your request is still approved; look at what changed and press **Publish** again.

If an article in your request was edited after it was approved, the request is out of date (*Edited since pinned: refresh*), and it can't be published until it is refreshed and reviewed again ([Publish requests and review](/editors/review/)).

## After you publish

The release is in the CMS at once; the website may take longer. If the project has website connections (rebuild the site, clear the CDN, check the site), Colophon's background worker works through them, and the release's label follows what they report. [Receipts: is it on the website?](/editors/receipts/) explains each label and what to do about it.

## Coming later

- **One-click publishing** from an article: one button that makes the publish request for you, and publishes it at once when your project lets people review their own work, or asks for review otherwise. <Status building="3.7" />
- **Scheduled publishing** at a time you choose, in your time zone, rechecked when it is due and shown as *delayed* if it runs late. <Status building="e/schedules" />
- **Rollback** as a new release that points at older content, so even going back moves the numbers forward. <Status planned="P4" />

## Technical details

Publishing is `POST …/changesets/{changeset}/publish` with `{"base_generation": N}`, the release the publisher had seen, plus `If-Match` with the changeset's hash and an `Idempotency-Key`. It commits the release and its delivery jobs in one transaction and answers `201` with the new `generation`; if another release committed first, it is `409` with the reason `generation_moved`. A release's number is its **generation**, per environment (`production`). [Releases and generations](/concepts/releases-and-generations/) and [Publish requests and reviews](/reference/api/publish-requests/) have the rest.
