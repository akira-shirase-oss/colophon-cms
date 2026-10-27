A release being in the CMS doesn't mean the website shows it. A site may need rebuilding, a CDN may still hold old copies, a webhook may have failed. So each release gets **one label**, worked out from what actually happened, and the label never claims more than the evidence shows.

## The four labels

| Label | What it means | What to do |
|---|---|---|
| **Published in CMS** | The release committed and the public API serves it. Any webhooks and purges have finished, but nothing checked the website itself: the project has no probe. | Nothing, or ask a developer to add a probe so the label can say more. |
| **Website update pending** | The website connections are still working on it: a webhook on its way, a CDN purge not yet accepted, or a probe that hasn't seen the release yet. | Wait. The label settles by itself; probes look again every few seconds for a while. |
| **Verified at 1 location** | A probe saw this release, or a later one, on the live site, from one place. The number is how many distinct places saw it. | Nothing. Your readers at that place see it. |
| **Needs attention** | A connection gave up: a webhook refused, a purge failed, a probe never saw the release. | Open the release's receipt to see which part failed and why, then tell whoever runs the website. |

"Verified" always comes with its number of locations. A local install has one, so it says **Verified at 1 location**: one probe, from one machine, at one moment. It doesn't mean every reader everywhere sees the release; it means that place did.

## The receipt

Press a release's label to open its **receipt**. It lists each website connection and how its part went (*delivered*, *purge accepted*, *saw this release*, *still looking*, *on its way*, *failed*, or *covered by release N* when a later release did the job), and every look a probe took, oldest first: when, from where, which release it saw, and what went wrong if anything did.

A later release can cover an earlier one: if release 8's rebuild finished, the site has release 7's changes too, so release 7's pending parts count as done.

## Why "purge accepted" isn't "gone"

A CDN that accepts a purge request has agreed to do it; it hasn't proved the old copies are gone. The Studio says *CDN purge accepted*, never "cleared". Only a probe looking at the site can say what readers get, and only from where it looked.

If a purge fails for good, copies the CDN holds can be served until they expire: up to 30 seconds for the latest content, up to 5 minutes for older releases. The release says so.

## Who sets this up

Website connections live under **Settings** (the Studio calls them *Destinations*), and only owners and developers can add or switch them off. [Webhooks and website connections](/reference/webhooks/) is the technical reference.
