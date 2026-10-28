<script>
	import Status from '#lib/components/Status.svelte';
</script>

Nothing reaches readers without a review. You gather what you want to publish into a **publish request**, send it for review, and an owner or publisher approves it. The approval counts only for exactly what they looked at.

## Asking for review from an article

The quickest way is the button beside an article you are editing. When your project lets people approve their own work and your role may approve and publish, it says **Publish** (or **Publish changes**, for a live article you have edited) and does everything in one click ([Publishing](/editors/publishing/)). Otherwise it says **Ask for review**: it makes a publish request holding the saved version of this article and sends it for review, then says *Sent for review*, with a link to the request. <Status done="3.7" />

## Making a publish request for several articles

1. Open **Publish requests** in the sidebar and press **New publish request**.
2. Give it a **Title**, for example *Spring launch*, then tick the articles to publish. Each one holds its **current version**: later edits don't sneak in.
3. To take an article off the website instead, tick **Unpublish** beside it. That holds the version readers see now, so the reviewer approves taking down exactly that. [Taking something down](/editors/taking-down/) has more.
4. Press **Create publish request**. A request holds 1 to 100 articles.

Anyone in the project who can write may make a request and send it for review: editors too.

## Sending it for review

Open the request and press **Ask for review**. Its state becomes *Waiting for review*. Along the top, the page shows how far it has come: *Prepared*, *Sent for review*, *Approved*, *Live*.

For every article, the page shows what changes for readers: a word-level comparison from the version that is live to the one in the request, marked *new*, *update* or *unpublish*. Press **Changes** to fold one away or open it again.

## Reviewing

Owners and publishers review. On the request's page they see the **Review** panel, *You approve exactly what is shown here. If it changes, your approval no longer counts.*, with an optional comment and two choices:

- **Approve this version**;
- **Request changes**, with a comment saying what.

The latest decision counts; earlier ones stay in the request's **History**, marked *on an earlier version* once the request changes. You may approve your own request: the request then says *approved their own request*, and its history says *who sent it: their own request*, so everyone can see it. A rule that requires a different reviewer exists, but is off until projects have settings.

## What makes an approval stop counting

An approval is bound to the exact versions in the request, by a fingerprint of them. So:

- **Someone edits an article in the request.** The request's state becomes *Edited since: update it*, and the article is marked *edited since it was added*. Press **Update to the latest versions** to put the new versions in; that changes the fingerprint, so it needs a fresh review. Nothing that wasn't reviewed can go out.
- **Seven days pass.** The approval expires (*Approval expired*) and must be given again.
- **The request changed while you were looking at it.** If someone updated or reviewed it in another tab, the page says *This publish request changed since you loaded it. Here is the current one: check it, then try again.* instead of acting on what you saw before.

## The states, in one list

| State | Meaning | What to do |
|---|---|---|
| Not sent for review | Made, not sent for review yet. | **Ask for review**. |
| Waiting for review | Sent; waiting for an owner or publisher. | Wait, or ask a reviewer. |
| Changes asked for | The reviewer asked for changes; their comment says what. | Edit the articles, then **Update to the latest versions**: the request is back to *Waiting for review*. |
| Approved | Approved on these exact versions. | An owner or publisher can publish it, now or at a time they choose. |
| Edited since: update it | An article changed after it was added. | **Update to the latest versions**; it then waits for a fresh review. |
| Approval expired | Approved more than seven days ago. | Review it again. |
| Published | It went out as a release. A request publishes once. | Nothing. |

The project's overview lists requests in progress under **In review**, and the number waiting under **Waiting for review**; the sidebar shows the same number beside **Publish requests**.

## Coming later

- Review stages, assignees and comments on fields. <Status planned="E" />

## Technical details

A publish request is a `changeset`: `POST …/changesets` with `{"title", "entries": [ids], "unpublish": [ids]}`, then `…/submit`, `…/refresh` and `…/reviews` with `{"decision": "approve"}` or `"request_changes"`. Every step after creation sends `If-Match` with the changeset's `hash`; a stale one is `409` with the reason `stale_changeset`. An approval counts while the changeset keeps the hash it was given on, for seven days. [Publish requests](/concepts/publish-requests/) and [Publish requests and reviews](/reference/api/publish-requests/) have the rest.
