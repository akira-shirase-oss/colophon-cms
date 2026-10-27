+++
title = "Checkpoint 3.2: taking it back"
description = "Colophon can now unpublish. A takedown is reviewed like any change, the server stops serving the article at once, even through links to old releases, and every CDN in front of it is told to drop its copies. Until a CDN confirms, the release says the removal is still pending."
date = 2026-10-26T17:07:26Z

[extra]
kind = "release"
milestone = "P3"
step = "3.2"
short_title = "Taking it back"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 241

[extra.summary]
goal = "Take published content back down: refused by the server at every release, purged from CDNs, and honest about what might still be out there."
built = "Unpublishing through review, tombstones that old release links respect, cache keys that retire on a takedown, and purge jobs for CDNs."
proof = ["a cached article is gone from every release after a takedown", "a fake CDN keeps its copy until the purge lands, and the release says so", "a million publishing requests, 20,000 takedowns, every purge accepted"]
code = "akira-shirase-oss/colophon-cms · main @ f15b0d7"
+++

Publishing was the easy direction. Every release is numbered and every old release can still be read, which is
exactly what makes taking something back hard: the article is in release 7's history, in release 7's cache entries,
and in whatever copies a CDN made of release 7. This step makes it disappear from all of them, or says plainly where
it hasn't yet.

## What I built

- **Unpublishing is a change like any other.** A changeset can now say "take this article down" as well as "publish
  this version". It pins the version that's live, so the reviewer approves taking down exactly what readers see. If
  someone publishes a newer version in the meantime, the takedown goes stale and has to be refreshed and reviewed
  again.
- **Gone from every release, not just the latest.** Publishing the takedown leaves a tombstone, and the server stops
  serving that article at any release number: lists leave it out, and asking for it directly answers *410 Gone*. A
  link to release 3 can't bring it back. If the article is published again later, it's served from that new release
  on, and the old links stay dead.
- **No cached copy survives the takedown.** Colophon's own cache files answers by release, and a release's answers
  never change, except after a takedown. So every cache name now also includes the number of the latest takedown: a
  takedown quietly retires every answer cached before it.
- **CDNs are told what to drop.** A project can add a CDN next to its webhooks. After every release, Colophon sends
  the CDN a purge listing the tags of what changed: each article's own tag, which every copy of it carries whatever
  release it came from, and its collection's, which every list carries.
- **Removal pending, until it isn't.** Each release now says whether its purge was accepted, is still pending, or
  failed. For a takedown, the Studio says *removal pending at the CDN* until the CDN accepts.

## How it works (the short version)

The tricky part was a rule from step 2.3: if release 5 is still waiting to be announced when release 6 arrives,
release 5's announcement is skipped, because 6 replaces it. That's right for a webhook. For a purge it would be a
disaster: if release 5 was the takedown, skipping its purge leaves the article in the CDN. So a purge doesn't cover
one release. It covers every release since the last purge that CDN accepted. Skipping one loses nothing, because
the next one includes it.

## Does it actually work?

- **A takedown with the cache warm (F07):** the article's list, its page and its page at release 1 are all cached.
  After the takedown, the lists come back without it, read afresh rather than from the old cache, and the article
  is *410 Gone* at the latest release, at release 1 and at release 2, never cached. Published again, it's back from
  release 3 only.
- **A fake CDN that really caches:** it keeps copies for as long as Colophon's headers allow and drops every copy
  carrying a purged tag. With the CDN refusing purges, the takedown is refused by the server at once, but the CDN
  keeps serving its copy and the release says *pending*. Once the CDN accepts, its copy is gone and reading through
  it gives 410.
- **A skipped purge is covered:** with the CDN refusing, a takedown and then another release go out; the takedown's
  purge is skipped, and the next one names both releases' articles.
- **A refused purge is reported:** a CDN that refuses for good leaves the release marked *failed*, not silently
  "done".
- **Review is enforced:** you can't unpublish something that isn't published, or publish and unpublish the same
  article in one changeset, and a takedown that went stale can't be published until it's refreshed and approved.
- **Publishing under load:** 1,000,328 requests (200,000 publishes, one in ten also taking an article down) at 912
  a second on my laptop, the slowest 1% under 146 ms, none failed. The worker sent 181,731 announcements and purges,
  none twice, and at the end every release's purge was accepted, including those skipped for a newer one.
- **Reading under load, twice:** a million public reads, a tenth of them for an article that was taken down (410
  every time). The first run did 6,016 a second, the slowest 1% under 22 ms, but a handful of requests took 12
  seconds, with no warning from the server, while the laptop was swapping hard. Run again straight after: 6,675 a
  second, slowest 1% under 20 ms, slowest request 108 ms. I kept both results; I can't explain the first one yet.
- **241 Rust tests,** 19 Studio tests against the server, 20 testbed tests, and CI passed every
  job ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40362147230)).

## Where I messed up

- **I nearly lost purges to my own rule.** My first plan for CDN purges reused the announcement jobs as they were.
  Writing down how a takedown would flow through them, I saw that the skip-an-older-release rule above would throw
  away exactly the purge that mattered. I caught it on paper. Purges now cover everything since the last one that
  worked, and a test checks the skipped case.
- **"Gone" said gone too often.** The first check for *410 Gone* asked "was this article ever taken down at or
  before this release?". Publish it again, and it was still "gone" at the new release. My own database test caught
  it. Gone now means nothing is served at that release *and* a takedown explains why.
- **A wrong test, for once not wrong code.** My cache test expected a read pinned to the newest release to miss the
  cache after a takedown. It hit, correctly: it's the same answer as the latest read just before it, under the same
  name.
- **My branches would have broken each other's database.** Each step's branch adds database changes the others
  don't have, and they all used one local database. Running this step's tests would have upgraded it under the
  other branches, whose next run would then refuse to start. A branch can now name its own database, and its tests,
  load tests and browser tests all use it.
- **A load test reported the wrong database.** The first run with a separate database said it made 3 database
  transactions in 10,000 requests: it was counting the shared one. Fixed before it went into a result.
- **Two checkboxes, one name.** The changeset form gives every article a *publish* and an *unpublish* box, and the
  first version called them both by the article's title. The browser tests couldn't tell them apart, and neither
  could a screen reader. The second is now *Unpublish (title)*.

## What it doesn't do yet

- A takedown waits for a reviewer. There's no emergency path yet, and truly instant removal would need every read
  to check permission with no shared cache at all.
- *Accepted* is the CDN's word that it took the purge, not proof the copies are gone. Checking that from outside is
  the next step.
- Purge endpoints have no authentication yet: the fake CDN needs none. The real Cloudflare connection, with its key
  kept out of the database, comes with my own site.
- Copies already in readers' browsers or downloads can't be recalled by anyone. A purge that fails for good leaves
  CDN copies until they expire: 30 seconds for the latest release, 5 minutes for an old one.

## Up next

**3.3, a receipt for every publish:** checking from outside that a release really reached the site, and showing it
as a receipt instead of a spinner.
