<script>
	import Status from '#lib/components/Status.svelte';
</script>

Publishing turns an approved publish request into a **release**: one numbered step in the website's history. Everything in the request goes live together, at the same moment, or nothing does. There is no half-published state for a reader to see.

## Publishing an approved request

Owners and publishers see a **Publish** button on an approved request. Press it, and the release is committed at once: the public API serves the new versions from that moment, and the request's state becomes *Published*. A request publishes once.

Releases are numbered 1, 2, 3 and so on, per project environment, and the numbers only go up. **Releases** in the sidebar lists them, newest first: what each changed, who published it, when, and its website label.

## When someone else published first

Each publish is made on top of the latest release your page had seen. If a colleague published another release in the meantime, yours is refused rather than quietly stacked on something you never looked at. The Studio says so: *Another release was published first. Check what is live now, then publish again.* Your request is still approved; look at what changed and press **Publish** again.

If an entry in your request was edited after it was approved, the request is out of date, and publishing is refused the same way: refresh it and get it reviewed again ([Asking for review](/editors/review/)).

## After you publish

The release is in the CMS at once; the website may take longer. If the project has website connections (a webhook that rebuilds the site, a CDN to clear, a probe that checks the site), Colophon's background worker works through them, and the release's label follows what they report. [What the website labels mean](/editors/website-status/) explains each label.

## Coming later

- **One-click publishing** from an entry: one button that makes the publish request and publishes it when the project lets you review your own work, or asks for review otherwise. <Status planned="P3" />
- **Scheduled publishing** at a time you choose, with time zones and daylight-saving handled. <Status planned="P4" />
- **Rollback** as a new release that points at older content, so even going back moves the numbers forward. <Status planned="P4" />
