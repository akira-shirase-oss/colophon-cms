<script>
	import Status from '#lib/components/Status.svelte';
</script>

Publishing in Colophon has three steps: gather what you want to publish into a **publish request**, have it reviewed, and publish it as a numbered **release**. Each release then ends with a **receipt**: one label saying how far the evidence goes, and the evidence itself. This page publishes the article from [Your first article](/get-started/first-article/), after connecting a probe so the receipt can say *Verified at 1 location*.

Today's Studio still calls a publish request a **changeset**, and website connections **destinations**; those screens get plain words in step 3.7. <Status building="3.7" />

## 1. Connect a probe first

A **probe** is a website connection that looks at an address after each release and reads which release the answer was built from. Without one, nothing checks the website, and a release can only say *Published in CMS*. Add it before you publish: a release delivers to the connections that exist when it is committed.

Only owners and developers manage website connections.

1. Open **Settings**, and copy the **Project id** (the copy button is beside it).
2. Under **Destinations**, choose the kind **Probe: checks the site shows each release**.
3. Leave **Location** empty, and give it a **Destination name**, for example *Public API*.
4. For the **URL to check**, use the project's public API address, `http://localhost:58000/content/v1/PROJECT_ID/production/articles` with your project's id in place of `PROJECT_ID`.
5. Press **Add destination**. It is listed with its switch, **Check each release**, on.

The form asks for `https`. A server listening on a loopback address, like the local one, also accepts `http` and private addresses, so a probe can reach `localhost`; on any other address only public `https` addresses are accepted.

Be clear about what this probe proves. Colophon's public API names its release in a `Colophon-Generation` header on every answer, so the probe proves that Colophon's own API serves the release, seen from the machine the worker runs on. A real website proves more: point the probe at one of its pages that sends the same header or carries `<meta name="colophon-generation" content="N">`, as the reference website in `scripts/demo.sh` does. Other kinds of connection rebuild the site (**Webhook**) or clear a CDN (**CDN**); [Webhooks](/reference/webhooks/) covers them.

## 2. Make a publish request

1. Open **Changesets** in the sidebar and press **New changeset**.
2. Give it a **Title**, for example *First article*, and tick the article. Each entry is pinned at its current version: later edits don't slip in.
3. Press **Create changeset**. Its page shows the progress, *Pinned*, *Submitted*, *Approved*, *Published*, and, for each entry, what changes for readers.
4. Press **Submit for review**. The state becomes **Awaiting review**.

A publish request holds 1 to 100 entries. [Publish requests and review](/editors/review/) explains every state.

## 3. Review it

Owners and publishers review. On the request's page they see the **Review** panel: *You approve version …, exactly as shown*, an optional comment, **Approve this version** and **Request changes**.

Signed in as `dev-owner`, you may approve your own request; the Studio marks it *reviewed their own changeset*. To see the two-person path, sign in as `dev-editor` in a private window (invited as an editor), make and submit the request there, and approve it as `dev-owner`.

An approval counts only for the exact versions in the request. If anyone edits a pinned entry afterwards, the request says *Edited since pinned: refresh* and needs a fresh review.

## 4. Publish

On an approved request, owners and publishers see **Publish**. Press it: the release is committed in one transaction, and the request's state becomes **Published**, *as release 1*. The public API serves the new versions from that moment.

Releases are numbered 1, 2, 3 and so on, per project environment, and the numbers only go up: even a future rollback will be a new, higher number. If someone else published in the meantime, yours is refused with *Another release was published first. Check what is live now, then publish again.* [Publishing](/editors/publishing/) has the details.

Publishing one entry with a single button, without making the request by hand, is being built. <Status building="3.7" />

## 5. Read the receipt

Open **Releases**. Release **No. 1** is listed with what it changed, who published it, when, and its label. The sidebar's foot says *Live now* with the same number and label.

Press the label to open the release's **receipt**, *Release 1: First article*. While the worker is still checking, the label is **Website update pending** (*On its way: the label settles by itself*), and the page looks again every two seconds. Within a few seconds it settles:

- **Verified at 1 location**: *A probe read this release (or a later one) on the live site.* Under **Destinations**, the probe says *saw this release*; under **What the probes saw**, each look is listed, oldest first: *From local (Public API): release 1, HTTP 200*.
- **Needs attention**, if the probe gave up: the receipt names what failed and why.

`local` is the worker's location, `COLOPHON_WORKER_LOCATION`, which defaults to `local`. One location means one probe, from one machine, at one moment: it doesn't mean every reader everywhere sees the release. [Receipts](/editors/receipts/) explains every label and what to do about each, and [Receipts and probes](/concepts/receipts-and-probes/) the rules behind them.

## The same through the API

The CLI has no publishing commands yet, but its token works with every route. `colophon login` keeps it in `credentials.json` (by default in `~/.config/colophon/`); a bearer token needs no CSRF header. Each step below is one request under `{P}`, which stands for `/api/v1/projects/{project}/environments/production`:

```text
POST {P}/changesets                        Idempotency-Key: …   {"title": "First article", "entries": ["ENTRY_ID"]}
POST {P}/changesets/{changeset}/submit     If-Match: "sha256:…"
POST {P}/changesets/{changeset}/reviews    If-Match: "sha256:…"  {"decision": "approve", "comment": "Ready."}
POST {P}/changesets/{changeset}/publish    If-Match: "sha256:…"  Idempotency-Key: …   {"base_generation": 0}
```

The `If-Match` value is the request's `hash`, quoted, which every answer about it returns; `base_generation` is the release you looked at (0 before the first). Publishing answers `201 Created` with the new `generation`. [Publish requests and reviews](/reference/api/publish-requests/) has every field and refusal.

Read the receipt with `GET {P}/releases/{generation}`:

```sh
TOKEN=$(jq -r .profiles.default.token ~/.config/colophon/credentials.json)
curl -s -H "Authorization: Bearer $TOKEN" \
  http://localhost:58000/api/v1/projects/PROJECT_ID/environments/production/releases/1 | jq
```

```json
{
  "data": {
    "generation": 1,
    "epoch": 1,
    "title": "First article",
    "published_by_name": "Dev Owner",
    "entries": 1,
    "unpublished": 0,
    "purge": "none",
    "label": "verified",
    "locations": 1,
    "deliveries": [
      { "name": "Public API", "kind": "probe", "location": null, "state": "done", "status": "succeeded", "attempts": 1 }
    ],
    "observations": [
      { "location": "local", "observed_generation": 1, "http_status": 200, "error": null }
    ]
  }
}
```

The answer is trimmed here: it also has the release's changeset, hash, publisher and time, and ids and times on each delivery and observation. `label` is one of `published`, `pending`, `verified` and `needs_attention`. [Releases and receipts](/reference/api/releases/) lists every field.

## Read what was published

Anyone can read published content, without signing in, from the public delivery API:

```sh
curl -i http://localhost:58000/content/v1/PROJECT_ID/production/articles
```

The answer names its release in `Colophon-Generation: 1`, carries an `ETag` and a cache policy (`public, max-age=0, must-revalidate, s-maxage=30` for the latest release), and lists each published entry with its `title`, `content`, `content_hash` and `published_in`, the release that published it. Drafts never appear here. Add `?generation=1` to read the collection exactly as release 1 had it, now or in a year; add the entry's id to the path to read one entry. [Delivery API](/reference/delivery-api/) has the rest.

## Next

- [Taking something down](/editors/taking-down/): unpublishing goes through the same review.
- [Releases and generations](/concepts/releases-and-generations/): why two releases on the same base can't both commit.
