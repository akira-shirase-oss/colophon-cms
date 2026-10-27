<script>
	import Status from '#lib/components/Status.svelte';
</script>

Nothing reaches readers without a review. You gather what you want to publish into a **publish request**, send it for review, and an owner or publisher approves it. The approval counts only for exactly what they looked at.

Today's Studio still calls a publish request a **Changeset**, in the sidebar and on its pages; step 3.7 renames it. <Status building="3.7" />

## Making a publish request

1. Open **Changesets** in the sidebar and press **New changeset**.
2. Give it a **Title**, for example *Spring launch*, then tick the articles to publish. Each one is pinned at its **current version**: later edits don't sneak in.
3. To take an article off the website instead, tick **Unpublish** beside it. That pins the version readers see now, so the reviewer approves taking down exactly that. [Taking something down](/editors/taking-down/) has more.
4. Press **Create changeset**. A request holds 1 to 100 articles.

Anyone in the project who can write may make a request and send it for review: editors too.

## Sending it for review

Open the request and press **Submit for review**. Its state becomes *Awaiting review*. Along the top, the page shows how far it has come: *Pinned*, *Submitted*, *Approved*, *Published*.

For every article, the page shows what changes for readers: a word-level comparison from the version that is live to the one in the request, marked *new*, *update* or *unpublish*. Press **Changes** to fold one away or open it again.

## Reviewing

Owners and publishers review. On the request's page they see the **Review** panel, *You approve version …, exactly as shown*, with an optional comment and two choices:

- **Approve this version**;
- **Request changes**, with a comment saying what.

The latest decision counts; earlier ones stay in the request's **History**, marked *on an earlier version* once the request changes. You may review your own request; the Studio shows *reviewed their own changeset* beside the approval, so everyone can see it. A project setting to require a different reviewer exists in the rules, but is off until project settings arrive.

## What makes an approval stop counting

An approval is bound to the exact versions in the request, by a fingerprint of them. So:

- **Someone edits a pinned article.** The request shows *Edited since pinned: refresh*, and the article is marked *edited since pinned*. Press **Refresh to the current versions** to pin the new versions; that changes the fingerprint, so it needs a fresh review. Nothing that wasn't reviewed can go out.
- **Seven days pass.** The approval expires (*Approval expired*) and must be given again.
- **The request changed while you were looking at it.** If someone refreshed or reviewed it in another tab, the page says *This changeset changed since you loaded it. Here is the current version: check it, then try again.* instead of acting on what you saw before.

## The states, in one list

| State | Meaning | What to do |
|---|---|---|
| Not submitted | Made, not sent for review yet. | **Submit for review**. |
| Awaiting review | Sent; waiting for an owner or publisher. | Wait, or ask a reviewer. |
| Changes requested | The reviewer asked for changes; their comment says what. | Edit the articles, then **Refresh to the current versions**: the request is back to *Awaiting review*. |
| Approved | Approved on these exact versions. | An owner or publisher can publish it. |
| Edited since pinned: refresh | An article changed after it was pinned. | **Refresh to the current versions**; it then awaits a fresh review. |
| Approval expired | Approved more than seven days ago. | Review it again. |
| Published | It went out as a release. A request publishes once. | Nothing. |

The project's overview lists requests in progress under **In review**, and the number waiting under **Waiting for review**.

## Coming later

- Publishing one article with a single button: the request is made for you, and published at once when your project lets people review their own work, or sent for review otherwise. <Status building="3.7" />
- Review stages, assignees and comments on fields. <Status planned="E" />

## Technical details

A publish request is a `changeset`: `POST …/changesets` with `{"title", "entries": [ids], "unpublish": [ids]}`, then `…/submit`, `…/refresh` and `…/reviews` with `{"decision": "approve"}` or `"request_changes"`. Every step after creation sends `If-Match` with the changeset's `hash`; a stale one is `409` with the reason `stale_changeset`. An approval counts while the changeset keeps the hash it was given on, for seven days. [Publish requests](/concepts/publish-requests/) and [Publish requests and reviews](/reference/api/publish-requests/) have the rest.
