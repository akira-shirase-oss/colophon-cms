A release being in the CMS doesn't mean the website shows it. A site may need rebuilding, a CDN may still hold old copies, a connection may have failed. So each release gets **one label**, worked out from what actually happened, and the label never claims more than the evidence shows.

## Where you see it

- On **Releases**, beside each release.
- At the foot of the sidebar, under *Live now*, for the release readers have now.
- On the project's **Overview**, under *Live release*.

Press a label to open the release's **receipt**.

## The four labels

| Label | What it means | What to do |
|---|---|---|
| **Published in CMS** | The release is made and the public API serves it. Any rebuilds and CDN clears finished, but nothing checked the website itself: the project has no check of the site. | Nothing, or ask whoever manages website connections to add a check of the site, so the label can say more. |
| **Website update pending** | The website connections are still working on it: a rebuild on its way, a CDN clear not yet accepted, or a check that hasn't seen the release yet. | Wait. The label settles by itself; checks look again every few seconds for a while. |
| **Verified at 1 location** | A check of the site saw this release, or a later one, on the live site, from one place. The number is how many different places saw it. | Nothing. Readers at that place see it. |
| **Needs attention** | A connection gave up: a rebuild refused, a CDN clear failed, a check never saw the release. | Open the receipt to see which part failed and why, then tell whoever runs the website. |

"Verified" always comes with its number of places. A local install has one, so it says **Verified at 1 location**: one check, from one machine, at one moment. It doesn't mean every reader everywhere sees the release; it means that place did.

## Reading a receipt

The receipt's title is the release and its name, *Release 3: Spring launch*, with who published it and when, and how many articles it changed or took down. Under the label, a line explains it: *Nothing checks the website yet. Add a probe in Settings to verify what readers see.* when no website connection worked on this release, *On its way: the label settles by itself.* while pending, and *A probe read this release (or a later one) on the live site.* once verified. While the label is pending, the page looks again every two seconds.

Below are two lists. Today's Studio uses the older words for both, *Destinations* and *What the probes saw*.

**Each website connection's part** (*Destinations*): its name, its kind (*Webhook* rebuilds the site, *CDN* clears it, *Probe* checks it), and how its part went:

| It says | Meaning |
|---|---|
| delivered | The rebuild was asked for, and the site answered that it got it. |
| purge accepted | The CDN accepted the request to clear the old copies. |
| saw this release | The check read this release, or a later one, on the site. |
| on its way | Still being sent or retried. |
| still looking | The check hasn't seen this release yet, and will look again. |
| covered by release N | A later release did the job, which covers this one too. |
| failed, and the reason | It gave up. The reason is what the site or the CDN answered, or why nothing answered. |

**Every look a check took** (*What the probes saw*), oldest first: when, from which place, which release it saw (or *no release named*), and the site's answer, such as *HTTP 200*.

A later release covers an earlier one: if release 8's rebuild finished, the site has release 7's changes too, so release 7's unfinished parts count as done.

## The lines under each release

On **Releases**, each release can also have a line per website connection, and one for the CDN:

| It says | Meaning |
|---|---|
| waiting, sending, will retry | Still being sent. *after 3 attempts* says how many tries it took. |
| delivered | Done. |
| skipped: a newer release went instead | A later release was sent in its place, which covers this one. |
| failed, and the reason in brackets | It gave up. |
| CDN purge pending: copies the CDN holds may still be served | The CDN hasn't accepted the request to clear its copies yet. |
| Removal pending at the CDN: copies it holds may still be served | The same, for a release that took something down. |
| CDN purge failed: copies the CDN holds may be served until they expire (up to 5 minutes) | Clearing the CDN gave up; old copies run out on their own. |

## Why "purge accepted" isn't "gone"

A CDN that accepts a request to clear its copies has agreed to do it; it hasn't proved the old copies are gone. The Studio says *purge accepted*, never "cleared". Only a check of the site can say what readers get, and only from where it looked.

If clearing the CDN fails for good, the copies it holds can be served until they expire: up to 30 seconds for the latest content, up to 5 minutes for older releases. The release says so.

## Who sets this up

Website connections live under **Settings**, where the Studio still calls them *Destinations*, and only owners and developers can see, add or switch them off: their addresses may carry secrets. Everyone else sees *Only owners and developers see destinations*. [Your first publish and receipt](/get-started/first-publish/) adds a check step by step.

## Technical details

A release's label is `published`, `pending`, `verified` (with `locations`) or `needs_attention`, read with `GET …/releases/{generation}` together with each destination's `deliveries` and the probes' `observations`. A probe reads the release a page was built from in the `Colophon-Generation` header or a `colophon-generation` meta tag. Website connections are `destinations` of kind `webhook`, `cdn` or `probe`, worked through by the worker's jobs. [Receipts and probes](/concepts/receipts-and-probes/), [Releases and receipts](/reference/api/releases/) and [Website connections and jobs](/reference/api/website-connections/) have the rest.
