<script>
	import Status from '#lib/components/Status.svelte';
</script>

"Published" has to mean published. A fresh answer from the API doesn't prove the website rebuilt, and a CDN that accepted a purge hasn't proved anything about what readers get. So Colophon looks: a **probe** fetches a page and reads which release it was built from, and each release's **receipt** keeps what every destination did and every look a probe took. In the Studio's plain words a probe is the website connection that checks the site.

## How a probe knows the release

A probe is a destination (`kind: "probe"`) with a URL. Its job for a release fetches the URL and reads the release the answer came from:

- the `Colophon-Generation` header, as the delivery API sends it; or
- a marker in the first megabyte of an HTML page, which a static site can carry without headers of its own:

```html
<meta name="colophon-generation" content="42">
```

The marker must be exact (`colophon_domain::receipt::marker`), or it counts as no marker. Seeing the release, or a later one, passes: a later release includes the earlier one's changes. An older release, or none, is "not yet": the probe looks again no sooner than every 5 seconds, up to 10 attempts. An error status counts as an error even with a marker.

Every look leaves an **observation**: the release looked for, the worker's location, the generation seen, the HTTP status, the SHA-256 of the body and any error. It is written with the attempt's result, through the worker's lease.

## Locations

Each worker has a location, `COLOPHON_WORKER_LOCATION` (`local` by default). A probe can be pinned to one, and then only workers there run it. So "2 locations" means two vantage points, not two looks from one machine. A local install runs one worker, so its receipts say **Verified at 1 location**.

## One label per release

`colophon_domain::receipt::label` turns the destinations' states into one label:

1. **Needs attention** if any destination gave up, whatever else happened;
2. otherwise **Website update pending** if any is still working;
3. otherwise **Verified at N locations** if a probe saw the release at N distinct locations (only probes verify);
4. otherwise **Published in CMS**.

A destination counts as done for a release once one of its jobs at that release or a later one succeeded: a newer announcement, purge or sighting covers the older one. `GET …/releases/{generation}` is the receipt: the label, each destination's part (done, covered by a later release, pending or failed), and every observation. The editor's side is [Receipts: is it on the website?](/editors/receipts/).

## What a receipt can't say

A probe sees one URL, from one place, at one moment. It can't say that every page or every CDN edge updated, and the label never implies it. Evidence is scoped to the locations checked, which is why "verified" always carries its number.

## The reference site carries the marker

`apps/web`, the reference website, is built from one release with every read pinned to it, and every page carries the marker exactly once. Its `/colophon` page names the release the site was built from. Promoting a build is atomic and never goes backwards: an older build can't replace a newer one. On the step 3.6 branch its article pages move to `/articles/{slug}/` when an article has a URL name. <Status building="3.6" />

It runs locally today. Putting the maintainer's site on the internet, with its hosting and probe locations (O-012), is step 3.8. <Status planned="P3" />

## How it is tested

`crates/colophon-server/tests/receipts.rs` runs probes against a fake site that serves an older release and then catches up, with a probe pinned to a location and an error page that needs attention; `crates/colophon-server/tests/site.rs` runs the real reference site, whose receipt says the site shows the release; the domain's tests cover the marker and the label rules.

## Design

[Module 06, delivery](/design/modules/06-delivery/), [module 05, worker](/design/modules/05-worker/) and [module 15, web](/design/modules/15-web-blog/); decisions D-051 and D-052 in the [decisions](/design/decisions/). On GitHub: [docs/modules/06-delivery.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/06-delivery.md).
