+++
title = "Checkpoint 2.1: approving the exact change"
description = "Colophon gets review. A changeset pins the exact versions of the entries in it, an approval is tied to a fingerprint of exactly that set, and editing anything afterwards means it has to be approved again. Two people can't approve a version neither of them is looking at."
date = 2026-10-26T04:08:09Z

[extra]
kind = "release"
milestone = "P2"
step = "2.1"
short_title = "Approving the exact change"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 188

[extra.summary]
goal = "Make \"approved\" mean \"someone looked at these exact versions\", and make an approval of anything else impossible to use."
built = "Changesets that pin exact revisions, a fingerprint of each, reviews tied to that fingerprint, and Studio pages to submit, review and refresh them."
proof = ["an edit after approval always needs a new one", "the latest decision wins, even in the same second", "1,000,267 changeset requests, 0 failed"]
code = "akira-shirase-oss/colophon-cms · main @ ec7a3c2"
+++

P2 is about publishing exactly once. Before anything can be published, someone has to approve it, and "approve"
turned out to be the interesting word. Most systems approve an article. But an article keeps changing, so what
exactly did the reviewer say yes to?

## What I built

- **Changesets.** You pick some entries, and the changeset pins the exact version of each one, along with the version
  of its content type it was checked against.
- **A fingerprint for each changeset.** It's computed from exactly what's pinned. Change any entry and re-pin, and the
  fingerprint changes.
- **Reviews tied to the fingerprint.** "Approve" and "request changes" record the fingerprint the reviewer was looking
  at, and only count while the changeset still has it.
- **Stale means stale.** If someone edits an entry after it was pinned, the changeset says so. A reviewer can't
  approve it until someone refreshes it to the new versions, which gives it a new fingerprint, so earlier approvals
  stop counting.
- **Studio pages** to make a changeset, submit it, approve or ask for changes, and refresh it. Approving your own
  changeset is allowed, and it's always shown as such.

## How it works (the short version)

Every action on a changeset carries the fingerprint the person was looking at. If the changeset has changed since,
the server refuses and says what it is now; in the Studio, the page shows the new version and asks you to look again.
So a reviewer can't approve a version they never saw, even if someone else refreshed it a second earlier in another
tab.

Who decided last is settled by a counter, not a clock. Each review takes the next number in its changeset's sequence,
while holding a lock on that changeset, so "latest" means "recorded last", even when two decisions land in the same
second.

## Does it actually work?

- **End to end, two people:** an editor pins two articles and submits them; editors can't review; the owner asks for
  changes, then approves that exact fingerprint. The editor edits an article: the approval stops counting, nobody can
  review the old fingerprint, and a refresh gives the changeset a new one that needs a new review.
- **The rules on their own:** order doesn't change a fingerprint and every field does; decisions on other fingerprints
  are ignored; approvals expire after seven days; a project that requires a second reviewer won't count a
  self-approval.
- **The database holds the line:** reviews can't be edited or deleted, a changeset can't pin another entry's
  revision, and another project's changesets don't exist as far as you can tell.
- **In two browser tabs:** one tab refreshes the changeset after an edit; the other, still showing the old version,
  presses Approve and is told it changed, then shown the current one.
- **A million changeset requests:** reading, listing, reviewing and creating changesets, 1,000,267 of them at 1,461 a second, none failed (p99 59 ms); at a steady 1,000 a second, p99 15 ms.
- **188 Rust tests,** 14 Studio tests against the server, 16 testbed tests, and CI passed every job
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40323010264)).

## Where I messed up

- **I ordered decisions by the clock.** A reviewer asked for changes and then approved a moment later, and the test
  still said "changes requested": both had the same second, and the older one won the tie. A finer clock wouldn't
  have fixed it either, because a transaction that waits for the lock started before the one it waited for, so its
  timestamp is earlier even though it came second. Decisions now take a number in sequence.
- **My first load test measured a traffic jam.** Every simulated user reviewed the same changeset, so every review
  waited in one line for the same lock: 180 ms each, which says nothing about how fast a review is. Real reviews are
  spread over many changesets; now each simulated user has its own.
- **A space that disappeared.** A label in the testbed lost the space before "(earlier hash)", because Svelte trims the
  start of a conditional block. A browser test caught it.

## What it doesn't do yet

- Nothing can be published yet. That's next.
- No images or files in changesets: media doesn't exist yet, and when it does, its fingerprints join the changeset's.
- A project can't yet require a second reviewer. The rule exists and is tested; the setting to turn it on comes with
  project settings.
- No side-by-side comparison of before and after yet: there is no "before" until something has been published.
- Load numbers come from one laptop. They show the code holds up; they aren't a promise about production hardware.

## Up next

**2.2, one release, one number:** publishing an approved changeset as a single release, numbered, where two releases
racing for the same number can't both win.
