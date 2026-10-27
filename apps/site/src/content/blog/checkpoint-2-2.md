+++
title = "Checkpoint 2.2: one release, one number"
description = "Colophon can publish. An approved changeset becomes a numbered release in a single step, two releases racing for the same number can't both win, anyone can read what's published (including any earlier release, exactly as it was), and the code that serves the public reads through a database account that can't see drafts at all."
date = 2026-10-26T04:40:25Z

[extra]
kind = "release"
milestone = "P2"
step = "2.2"
short_title = "One release, one number"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 199

[extra.summary]
goal = "Turn an approved changeset into a release with exactly one number, and make it impossible for public reads to show a draft."
built = "Numbered releases committed in one transaction, a publication history that can't overlap, and a public read API for the latest or any earlier release."
proof = ["two releases on one base: exactly one commits", "public reads can't touch drafts, by permission", "1,000,058 public reads, 0 failed"]
code = "akira-shirase-oss/colophon-cms · main @ 2831ef2"
+++

This is the step the whole project is named for, in a way: a colophon is the note at the end of a book that says
exactly what was printed. Here, publishing finally exists, and every publish gets a number.

## What I built

- **Publishing.** An approved changeset gets a Publish button. Pressing it creates release 1, then 2, then 3: the
  numbers only ever go up by one.
- **One number per release, even under pressure.** Two people publishing at the same moment, both looking at release
  4, can't both become release 5. One wins; the other is told another release came first and asked to look again.
- **A public read API.** Anyone, signed in or not, can read what a project publishes, as of the latest release or as
  of any earlier one, exactly as it was then.
- **Drafts can't leak.** Public reads go through a database account that can read published content and nothing
  else. Even a bug in that code couldn't show a draft: the database would refuse.
- **A Releases page** in the Studio, showing each release, who published it, and where the public can read it.

## How it works (the short version)

A release is one database transaction. It bumps the environment's release number only if it still has the number the
publisher saw, records the release, and copies each newly published version into a separate published table, marking
when it started being live. When a later release replaces it, its "live until" is set to that release's number.

Those "from release A until release B" ranges are what make old releases readable: "what was live at release 3" is
just a question about ranges. The database itself refuses ranges that overlap for the same entry, so an entry can never
be two versions at once.

## Does it actually work?

- **End to end, against the real server:** an editor's two articles become release 1, readable by anyone with no
  sign-in. The editor saves a new draft: the public still sees the published version. Release 2 publishes the edit;
  asking for release 1 still shows the old text.
- **The race:** two approved changesets, published at the same instant on the same release number. Exactly one
  commits; the other gets "another release committed first" and the current number.
- **Paging stays consistent:** reading a long list page by page while a new release lands, the next page still comes
  from the release the first page was read at.
- **The database account behind public reads** is refused drafts, entries, changesets, members, sessions, people, and
  every kind of write.
- **In real browsers:** publish from the Studio, see it on the Releases page and in the public API; a second tab that
  still thinks release 1 is live is told another release came first, then publishes on top of it.
- **A million anonymous reads:** 1,000,058 public reads of the latest and earlier releases at 2,650 a second, none failed (p99 31 ms); at a steady 2,000 a second, p99 9 ms.
- **199 Rust tests,** 15 Studio tests against the server, 17 testbed tests, and CI passed every job
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40323015173)).

## Where I messed up

- **A test that waited for itself forever.** One database test kept a transaction open, then asked the test database
  to shut down, which waits for every connection to come back. It never would. The test hung until I killed it, and
  left its scratch database behind; I removed it and fixed the test.
- **"Publish" also matched "Read published articles".** A browser test asked for the Publish button and found two.
  Small, but it's the third time a test has tripped on a label meaning more than I thought, so now I write them exactly
  from the start.
- **A script that half-ran, and a commit that caught it halfway.** The documentation update stopped on one mismatched
  line, and the commit command after it still ran, so the commit held half the change. Nothing had left my machine;
  I finished it and amended that commit.

## What it doesn't do yet

- No caching yet: every public read goes to the database. Caching and CDN headers are the next milestone.
- Public addresses use the project's id, not its name; readable names come with caching.
- Everything published is public. Private projects come later.
- Publishing doesn't yet notify anything outside Colophon (a site rebuild, a webhook). That's the next step.
- Load numbers come from one laptop. They show the code holds up; they aren't a promise about production hardware.

## Up next

**2.3, work that survives a crash:** telling the outside world about a release exactly once, even if the server dies
halfway through.
