Nothing reaches readers without a review. You gather what you want to publish into a **publish request**, send it for review, and an owner or publisher approves it. The approval counts only for exactly what they looked at.

The Studio still calls a publish request a **Changeset**; the screen is renamed in step 3.7.

## Making a publish request

1. Open **Changesets** in the sidebar and press **New changeset**.
2. Give it a title, then pick the entries to publish. Each one is pinned at its **current version**: later edits don't sneak in.
3. To take an entry off the website instead, pick it with **Unpublish**. That pins the version readers see now, so the reviewer approves taking down exactly that. [Unpublishing](/editors/unpublishing/) has more.
4. Press **Create changeset**. A request holds 1 to 100 entries.

## Sending it for review

Open the request and press **Submit for review**. Its state becomes *Awaiting review*. For every entry, the page shows what changes for readers: a word-level comparison from the version that is live to the one in the request.

## Reviewing

Owners and publishers review. On the request's page they see the version under review, its comparison, and two choices:

- **Approve this version**, optionally with a comment;
- **Request changes**, with a comment saying what.

The latest decision counts. You may review your own request; the Studio records that it was a self-review. A project setting to require a different reviewer exists in the rules but is off until project settings arrive.

## What makes an approval stop counting

An approval is bound to the exact versions in the request, by a fingerprint of them (a hash). So:

- **Someone edits a pinned entry.** The request shows *Edited since pinned: refresh*. Press **Refresh to the current versions** to pin the new versions; that changes the fingerprint, so it needs a fresh review. Nothing that wasn't reviewed can go out.
- **Seven days pass.** The approval expires (*Approval expired*), and needs to be given again.
- **The request changed while you were looking at it.** If someone refreshed or reviewed it in another tab, the page shows the current version and says so, instead of acting on what you saw before.

## The states, in one list

| State | Meaning |
|---|---|
| Not submitted | Made, not sent for review yet. |
| Awaiting review | Sent; waiting for an owner or publisher. |
| Changes requested | The reviewer asked for changes; their comment says what. |
| Approved | Approved on these exact versions; it can be published. |
| Edited since pinned: refresh | An entry changed after it was pinned. Refresh, then review again. |
| Approval expired | Approved more than seven days ago. Review again. |
| Published | It went out as a release. A request publishes once. |
