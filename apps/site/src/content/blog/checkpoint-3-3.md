+++
title = "Checkpoint 3.3: a receipt for every publish"
description = "Every release now gets one honest label: published in the CMS, website update pending, verified at so many locations, or needs attention. Probes look for each release from outside and keep what they saw, so 'verified' always comes with evidence and says where from. Along the way, logging finally named a slowdown CI had been tripping over for two steps."
date = 2026-10-26T19:02:28Z

[extra]
kind = "release"
milestone = "P3"
step = "3.3"
short_title = "A receipt for every publish"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 248

[extra.summary]
goal = "Replace \"published\" with evidence: check from outside that a release reached the site, and say exactly where that was checked."
built = "Probe destinations that read the live release from a header or a page marker, observations kept for every look, one label per release, and a receipt page."
proof = ["verified at 2 locations takes two workers in two places", "an error page needs attention, even with the right marker", "a million publishing requests, every release verified at 1 location"]
code = "akira-shirase-oss/colophon-cms · main @ c5980d6"
+++

Until now, "published" meant the database said so. The release committed, the webhook got its announcement, the CDN
accepted its purge, and none of that proves a reader sees the new version. This step adds the missing piece: looking.

## What I built

- **Probes.** A project can add a probe next to its webhooks and CDNs: a URL to check after every release. The probe
  reads which release the answer came from: Colophon's public API says so in a header, and a page can say it in a
  `<meta name="colophon-generation">` tag, which is how my static site will prove which release it was built from.
  Seeing the new release, or a later one, passes. Seeing an older one means "not yet", and it looks again, at most
  every five seconds, up to ten times.
- **Every look is kept.** Each attempt stores what it saw: the release it was looking for, where it looked from, the
  release it found, the status, a fingerprint of the page, and any error. That's the evidence behind a receipt.
- **Locations, not just "verified".** A worker knows where it runs, and a probe can be pinned to a location: only a
  worker there runs it. "Verified at 2 locations" means two probes saw it from two places, not one probe looking
  twice.
- **One label per release.** Every release now shows one of four labels: *Published in CMS* (nothing checked the
  site), *Website update pending*, *Verified at N locations*, or *Needs attention* (something gave up). In the Studio
  each label opens the release's receipt: what each destination did, and every look the probes took, oldest first.

## How it works (the short version)

A destination counts as done for a release once one of its jobs, for that release or a later one, succeeded. That
sounds loose, but it's what each one actually does: an announcement of release 8 tells a site about 7's changes too,
a purge covers every release since the last one that worked, and a probe that sees release 8 has seen at least 7.
So a release can be verified by a later release's probe, and the receipt says so (*covered by release 8*) instead of
pretending its own probe ran.

Only probes can verify. A release delivered to every webhook and CDN, with nothing looking at the site, stays
*Published in CMS*.

## Does it actually work?

- **A probe that waits (receipt test):** the fake site keeps serving release 1 after release 2 is published. The
  release stays *pending*, and its first observation says "saw release 1, not yet 2". Once the site serves 2, the
  release is *verified at 1 location*, with every look on its receipt and the page's fingerprint recorded.
- **Two locations:** one probe runs anywhere, another is pinned to `eu-central`. With only the local worker running,
  the release waits at *pending* with 1 location. Starting a second worker that says it runs in eu-central makes it
  *verified at 2 locations*, and the receipt shows both places.
- **An error page proves nothing:** a 404 page carrying the right marker still makes the release *need attention*.
  Fix the site and publish again, and the new release's probe covers the old one too.
- **Under load:** 1,000,393 publishing requests at 941 a second on my laptop, with a probe of the project's own
  public API on every one of the 64 projects. None failed, the slowest 1% took under 202 ms, and at the end every
  listed release was verified at 1 location with its purge accepted. (That's the third run; the first two failed,
  below.) The entries list, rewritten this step, did a million requests at 1,746 a second with none failed.
- **248 Rust tests,** 20 Studio tests against the server, 21 testbed tests, and CI passed every
  job ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40381142660)).

## Where I messed up

- **I nearly changed the load tests on a guess.** After step 3.2, main's CI kept failing load checks on write paths:
  a few hundred milliseconds of stall, on fast machines, on code that passed elsewhere. One failure showed role
  changes queued on a lock, and I suspected the CI machine's disk was slow to flush commits. I could have turned off
  waiting for the disk in CI and called it fixed. Instead I made CI log every slow statement, slow commits and lock
  waits included, and waited for the next failure. It came, and it wasn't the disk: the list of entries was taking
  130 to 400 ms. Postgres had planned it with statistics from when the tables were nearly empty, expected two
  revisions, and checked all 1,024 revisions against all 512 entries to return 21, discarding 523,776 rows. The
  query now looks up each entry's draft on its own, a shape whose only plan stops after the page. The same check on
  the same kind of machine went from 924 ms to 19 ms. It also explains a failure in step 3.2 I'd put down to luck.
- **My own receipts didn't scale.** The million-request publishing test failed at its very last step, twice: one
  request out of a million timed out at the database's five-second limit. The second time I kept the server's log,
  and it named the query: the list of releases, which this step taught to say whether each one was delivered. I'd
  written "done once a job at this release or later succeeded" as a search through the later jobs, for every
  release, and Postgres ran it for all 3,139 releases of a project before keeping the newest 100: 14.8 million rows,
  5.2 seconds. The same rule is "the latest success is at this release or later", and each destination's latest
  job, success and open job can be read once by index. Same answers for all 100 releases, in 6.7 ms. The Studio's
  releases page would have hit the same wall on any project with a long history.
- **A test from the last step passed by luck.** One purge test checked that the CDN refused the first attempt. When
  the worker got a few milliseconds slower, sometimes it hadn't tried yet when the test switched the CDN back on, so
  the first attempt succeeded. It now waits until the CDN has actually refused once.
- **A name already taken.** I called the worker's location setting `COLOPHON_PROBE_LOCATION` before noticing that
  "probe" already meant the server's health checks (`COLOPHON_PROBE_INTERVAL_MS`). It's `COLOPHON_WORKER_LOCATION`
  now, before anyone depended on it.

## What it doesn't do yet

- One location by default: my setup runs one worker, so receipts will honestly say *verified at 1 location*. Where
  the site is hosted, and where more probes run from, gets decided with the site itself.
- A probe checks one URL at one moment. It can't say every page, or every CDN edge in the world, has the new release.
- The role-change stall from that one failed run hasn't been explained. CI now logs enough to say what it is if it
  happens again.

## Up next

**3.4, my own site, running on Colophon:** a real site built from Colophon's releases, with that page marker in
every page, and a probe watching it.
