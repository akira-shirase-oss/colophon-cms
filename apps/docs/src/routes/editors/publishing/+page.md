<script>
	import Status from '#lib/components/Status.svelte';
</script>

Publishing turns an approved publish request into a **release**: one numbered step in the website's history, with the time it went live. Everything in the request goes live together, at the same moment, or nothing does. There is no half-published state for a reader to see.

## Publishing in one click, from an article <Status done="3.7" />

Beside an article you are editing, the Studio offers one button. When your project lets people approve their own work and your role may approve and publish, it says **Publish** (or **Publish changes**, for a live article you have edited since). One click makes a publish request for the saved version, sends it for review, approves it as your own review, and publishes it as the next release; the button shows each step as it goes (*Sending it for review…*, *Approving it…*, *Publishing…*). Afterwards it says *Live as Release N. Its receipt shows what reached your website.* The request stays in **Publish requests**, with your approval in its history, as if you had pressed each button yourself.

Otherwise the button says **Ask for review** and stops after sending the request ([Publish requests and review](/editors/review/)). If your role can do neither, it says so. The button waits until your latest changes are saved, and when readers already see the saved version it says *Readers see this version* instead.

If another release went live while you were publishing, it stops and says so: *Another release went live while you were publishing. Your request is approved and waiting: look at what is live now, then publish it from the request.*

## Publishing an approved request

Owners and publishers see a **Publish** button on an approved request. Press it, and the release is made at once: the public API serves the new versions from that moment, and the request's state becomes *Published*, with a link to *release N*. A request publishes once.

Releases are numbered 1, 2, 3 and so on, per project, and the numbers only go up. **Releases** in the sidebar lists them, newest first, as *No. 1*, *No. 2* and so on: what each changed, who published it, how long ago, and its website label. The foot of the sidebar always shows the release readers have now, under *Live now*, or *Nothing live yet* before the first.

## When someone else published first

Each publish is made on top of the latest release your page had seen. If a colleague published another release in the meantime, yours is refused rather than quietly stacked on something you never looked at. The Studio says so: *Another release was published first. Check what is live now, then publish again.* Your request is still approved; look at what changed and press **Publish** again.

If an article in your request was edited after it was approved, the request is out of date (*Edited since: update it*), and it can't be published until it is updated and reviewed again ([Publish requests and review](/editors/review/)).

## Publishing later, at a time you choose <Status done="D-060" />

An approved request's page has a **Schedule** panel: *Publish this exact version later, at a time you choose.* Pick a **Date**, a **Time** and a **Time zone** (yours is first, marked *(yours)*) and press **Schedule**. Only people whose role may publish everything in the request can schedule it.

- **When the clocks change.** On the night daylight saving starts or ends, a time can happen twice or not at all. The Studio never guesses: it says *The clocks go back that night in …, so 01:30 happens twice. Which one?* (or that the time doesn't happen) and shows both moments; pick one and press **Schedule at this moment**.
- **While it waits**, the panel says when it publishes, in its time zone and, if yours differs, where you are, and who scheduled it. **Cancel the schedule** stops it. **Releases** lists what is scheduled above the releases.
- **When the time comes**, it publishes as the person who scheduled it, if their role still allows it and nothing in the request has changed. Afterwards the panel says *Published on time, as release N*, *Published late, as release N* (it started more than a minute after its time, for example because the server was down), *Not published:* with the reason, or *Cancelled*. A request edited after approval, an approval that expired, or a role that can no longer publish stops it, and nothing is published.

Cancelling never undoes a release that has already gone out. [Scheduled publishing](/concepts/scheduled-publishing/) explains the rules.

## After you publish

The release is in the CMS at once; the website may take longer. If the project has website connections (rebuild the site, clear the CDN, check the site), Colophon's background worker works through them, and the release's label follows what they report. [Receipts: is it on the website?](/editors/receipts/) explains each label and what to do about it.

## Coming later

- **Rollback** as a new release that points at older content, so even going back moves the numbers forward. <Status planned="P4" />

## Technical details

Publishing is `POST …/changesets/{changeset}/publish` with `{"base_generation": N}`, the release the publisher had seen, plus `If-Match` with the changeset's hash and an `Idempotency-Key`. It commits the release and its delivery jobs in one transaction and answers `201` with the new `generation`; if another release committed first, it is `409` with the reason `generation_moved`. One-click publishing is the same calls in a row: create, submit, review, publish. Scheduling is `POST …/changesets/{changeset}/schedule` with `local_time`, `time_zone` and, at a clock change, `choice`; the worker runs it as its author. A release's number is its **generation**, per environment (`production`). [Releases and generations](/concepts/releases-and-generations/) and [Publish requests and reviews](/reference/api/publish-requests/) have the rest.
