+++
title = "Checkpoint 1.2: a draft that can't be lost"
description = "Colophon now saves articles. Every save is a new revision that can never be changed, a save on top of someone else's is refused instead of overwriting it, and the server can be killed just before or just after a save without losing it or saving it twice."
date = 2026-10-25T23:26:49Z

[extra]
kind = "release"
milestone = "P1"
step = "1.2"
short_title = "A draft that can't be lost"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 133

[extra.summary]
goal = "Save an article so that nothing acknowledged is ever lost, and nothing is ever saved twice."
built = "Entries with revisions that can't be changed, saves that name the revision they edited, idempotency keys, and tests that kill the server in the middle of a save."
proof = ["killed before or after a save: nothing lost, nothing doubled", "a stale save is refused, never overwrites", "1,000,070 requests, every read matched its last save"]
code = "akira-shirase-oss/colophon-cms · main @ 00cfba4"
+++

Saving sounds like the easy part of a CMS. It's easy when nothing goes wrong. The hard cases are the ones that happen
on a bad day: two people editing the same article, a network that drops the server's answer, a server that dies halfway
through a save. This step is about those.

## What I built

- **Articles.** A title and a rich-text body, inside a project. (Content types you define come later; for now
  everything is an article.)
- **Every save is a new revision, and revisions never change.** The database itself refuses to edit or delete one:
  the application's own database user simply doesn't have permission. Each article keeps its full history.
- **Saves name the revision they started from.** If someone else saved in between, your save is refused with "the
  draft has changed since that revision", and nothing is written. Nobody's work is silently overwritten.
- **Every write carries a one-time key.** Send the same request twice, by accident or on purpose, and you get the same
  answer back. The save happens once.
- **An Entries page in the testbed,** and a load test that checks every single read.

## How it works (the short version)

A save is one database transaction: lock the article, check that it still points at the revision you edited, add the
new revision, move the pointer, record the answer under your key, commit. Only then does the server reply. So "saved"
always means it's actually in the database.

If that reply gets lost (a dropped connection, a crash), your app sends the same request again with the same key. The
server finds the recorded answer and sends it back, instead of saving a second time or telling you that your own save
conflicts with itself.

## Does it actually work?

- **I kill the server on purpose.** A special test build stops dead at exactly two moments: just before the save is
  committed, and just after. Before: the request gets no answer, nothing was stored, and retrying works. After: the
  save is there, the retry gets the same answer marked as a replay, and the history has each save exactly once.
  Creating an article survives the same crash: one article, not two.
- **Two saves at the same moment, on the same revision:** one waits for the other, exactly one wins, the other is told
  it's out of date, and its revision is never stored.
- **Two browser tabs, same article.** Tab two's save is refused; reloading shows tab one's text, and both revisions are
  in the history.
- **The database guards itself:** even the application's user can't edit or delete a revision, or point one article at
  another article's revision.
- **A million requests, a fifth of them saves:** 1,000,070 at 1,893 a second, none failed (p99 44 ms), and every one
  of the roughly 700,000 reads returned exactly the revision its writer had last saved.
- **133 Rust tests,** plus 12 testbed tests in a real browser, and CI passed every job ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40248086977)).

## Where I messed up

- **My database tests hung for ten minutes.** Each test closes its connection pool at the end, and closing a pool waits
  for every borrowed connection to come back. Three tests were still holding a transaction open, so they waited
  forever. Now they end it first, and the whole suite takes about two seconds.
- **Chicken and egg.** Colophon checks its SQL against the real database when it compiles, and its migration command is
  part of the same program. So a migration that adds tables can't run until the program compiles, and the program
  won't compile until the tables exist. For that one step I used the database library's own migration tool, and wrote
  the workaround down.
- **An off-by-one I caught before it shipped.** A list asks the database for one extra row, just to learn whether
  another page follows. But the database layer capped every request at 100 rows, so a full page of 100 could never say
  "there's more".

## What it doesn't do yet

- Only one kind of content, articles. Content types you define are step 1.5.
- No languages or URL slugs yet.
- The Studio's editor doesn't save yet. That's next, with autosave and a proper conflict screen.
- Nothing cleans up old one-time keys yet; they're kept, and a retention job will remove them later.
- These numbers come from one laptop running the database in Docker. They show the code holds up; they aren't a promise
  about production hardware.

## Up next

**1.3, two editors, one article:** the Studio's editor saves and autosaves, and when someone else got there first it
shows what changed instead of an error.
