+++
title = "Checkpoint 1.4: the simulator found a bug on its first day"
description = "A simulator that runs thousands of repeatable, randomized days of editors colliding, answers going missing and browsers crashing, checking after every step that no acknowledged save was lost. It caught every bug I planted, agreed with the real database save for save, and found a real one in the Studio."
date = 2026-10-26T00:02:01Z

[extra]
kind = "release"
milestone = "P1"
step = "1.4"
short_title = "The first simulator run"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 148

[extra.summary]
goal = "Turn rare timing bugs into something you can replay on demand, and prove the save rules hold under all of them."
built = "A deterministic simulator of editors, a lossy network and crashes; planted bugs it must catch; a check that its model matches the real database."
proof = ["2,000 seeded runs, 0 invariant failures", "4 of 4 planted bugs caught", "1 real bug found and fixed"]
code = "akira-shirase-oss/colophon-cms · main @ 542d0eb"
+++

The bugs that hurt a CMS most are timing bugs: an answer lost right after a save succeeded, two people saving within
the same second, a browser closed at exactly the wrong moment. They happen rarely, and you can't reproduce them from a
bug report. So instead of waiting for them, Colophon now goes looking.

## What I built

- **A simulator.** Imagine a small office: six editors in two projects, sharing a few articles. They type, save,
  collide, retry. In between, the simulator's network loses requests and answers, and editors' browsers crash and
  reopen. It plays out hundreds of moves like that, and after every single move it checks the rules.
- **The rules it checks.** No save an editor was told succeeded is ever lost or silently overwritten. No edit is ever
  stored twice. Nobody ever reads another project's article.
- **Repeatable chaos.** Every run comes from one number, the seed. The same seed plays out exactly the same way on any
  computer, so when something fails, you get a seed, and replaying it shows every step that led there.
- **Planted bugs.** Four deliberate bugs, one at a time: skip the "is this still the latest version?" check, ignore the
  one-time keys, let reads ignore the project, and one more about crashed browsers. A simulator that can't catch a bug
  I planted proves nothing.

## How it works (the short version)

The simulator doesn't run the real database. It runs a small model of it, with the same rules. That raises the obvious
question: what if the model is wrong? So a second test takes the simulator's runs and replays every save, in order,
against real PostgreSQL. Every answer has to match: saved, already saved, or out of date. And to make sure that test
isn't just agreeing with everything, it also replays a run from a deliberately broken model, and requires PostgreSQL to
disagree.

## Does it actually work?

- **2,000 runs of 400 moves each, in about two seconds:** almost 227,000 acknowledged saves, 80,000 conflicts, 72,000
  lost requests and answers, 13,700 browser crashes. Zero rule failures.
- **All four planted bugs caught**, each on the very first seed, each by the rule it should break.
- **The real database agrees** with the model on every save across four complete runs, over 500 saves.
- **The same seed gives the same run,** line for line, every time.
- **148 Rust tests,** CI runs a 2,000-run sweep on every change ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40248087544)).

## The bug it found

The fourth planted bug wasn't really planted. It described what the Studio actually did.

When you reopen an article after a crash, the Studio finds your unsaved words on your device and saves them. Here's the
sequence the simulator found: your save reaches the server and succeeds, but the answer is lost; the tab closes; a
colleague saves on top of your version; you reopen. Your device still says "unsaved", the article has moved on, so the
Studio showed the conflict screen. Choose "keep mine", and your words were saved a second time, quietly undoing your
colleague's change.

The fix: your device now also remembers the exact request it sent, one-time key included. Reopening sends that same
request again, and the server, recognising the key, answers "already saved" before anything else. A browser test now
reproduces the whole sequence; it fails on the old code and passes on the new.

## Where I messed up

- **My first idea for the main rule was too weak.** "Every acknowledged save is stored" can't catch an overwrite,
  because revisions are never deleted: the overwritten one is still stored. The rule that works is that every
  acknowledged save must still be in the article's history, counting back from the current version.
- **The regression test I first wrote wasn't one.** It passed on the old code too, because the old code handled that
  exact case. Only adding the colleague's save in the middle made it fail without the fix.

## What it doesn't do yet

- It models saves only. Releases, background workers and delivery join it in P2 and P3.
- No virtual clock yet: nothing in P1 depends on time. Leases and schedules will need one.
- No nightly sweep of millions of runs; every change gets 2,000.

## Up next

**1.5, content types you define:** schemas that replace the one built-in "article", so a site can have pages, posts and
whatever else it needs.
