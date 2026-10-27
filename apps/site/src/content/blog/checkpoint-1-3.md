+++
title = "Checkpoint 1.3: two editors, one article"
description = "The Studio now edits real drafts. It saves as you type, keeps your unsaved words on your own device, and when someone else saved first it shows both versions and lets you choose, instead of quietly overwriting anyone."
date = 2026-10-25T23:47:01Z

[extra]
kind = "release"
milestone = "P1"
step = "1.3"
short_title = "Two editors, one article"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 136

[extra.summary]
goal = "Make the editor itself safe: autosave that never lies, nothing lost offline, and a real choice when two people collide."
built = "The Studio served by the server, real projects and articles, autosave with a device copy of unsaved edits, and a conflict screen."
proof = ["two browsers, one article: nothing overwritten", "offline edits saved when the network returns", "0 accessibility violations on every new screen"]
code = "akira-shirase-oss/colophon-cms · main @ 41733b0"
+++

Until now, the editor was a demo: you could type, but nothing was saved anywhere. This step connects it to the real
server, and the interesting part isn't the saving. It's everything around it: what happens when the network drops, when
you close the tab too early, and when a colleague is editing the same article at the same moment.

## What I built

- **The Studio, served by the server itself.** One address for both, so signing in just works.
- **Projects and articles.** Pick a project, see its articles, create one, open it.
- **Autosave.** A second after you stop typing, the article is saved. The status line says exactly where things
  stand: *Saved*, *Saving…*, *Unsaved changes, kept on this device*, or *Can't reach the server; retrying*.
- **Your unsaved words live on your device first.** Close the tab before the autosave fires, lose the network, or let
  your session expire: the text is still there when you come back, and it gets saved.
- **A conflict screen instead of an overwrite.** If someone else saved first, you see their version and yours side by
  side, and choose: keep yours (theirs stays in the history) or use theirs.

## How it works (the short version)

Every save tells the server which version you started from. If that's no longer the latest, the server refuses, and
that refusal is what opens the conflict screen. Nothing gets overwritten without someone choosing to.

Every save also carries a one-time key. If the server's answer gets lost, the Studio sends the same request again with
the same key, and the server replies "already done" instead of saving twice.

## Does it actually work?

These tests run the real Studio in a real browser against the real server:

- **Two browsers, one article.** The second save opens the conflict screen showing both versions. Keeping mine gives
  three revisions in the history, nothing lost; using theirs shows their text and drops mine.
- **Offline.** The test cuts the browser's network, types, sees "retrying", restores the network, and the edit is saved.
- **A lost answer.** The test lets a save reach the server, then throws the answer away. The Studio retries with the
  same key, the server replays its answer, and the history has the edit once.
- **Closing too early.** Type, reload before the autosave: the words come back, marked as restored, and get saved.
- **Accessibility.** An automated scanner finds no problems on the home page, the editor, or the conflict screen.
- **The Studio's files under load:** 1,000,001 requests at 17,214 a second, none failed (p99 8 ms).
- **136 Rust tests,** 9 Studio tests against the server, 76 editor tests, 12 testbed tests, and CI passed every
  job ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40248087189)).

## Where I messed up

- **The history list went stale.** After choosing "use their version", the list still showed the old history: that
  tab had never saved, so nothing told it to refresh. A conflict now refreshes it.
- **The editor page had no main heading,** so a screen reader user had nothing to jump to. The accessibility scanner
  caught it on the first run.
- **A "Saving…" that could last forever.** If the server accepted a request and never answered, the status would
  spin indefinitely. Every request now gives up after 15 seconds and is retried with the same key.
- **My first steady load test for the Studio asked too much.** At 10,000 requests a second, rare 75 ms stalls left
  the load tool short of workers and it dropped 0.04% of its requests, past the 0.01% allowed. I kept that failed
  result and run the test at 5,000, still far more than editors loading the Studio will ever need.
- **A short load test was flagged as a regression when the comparison was the problem.** A 10,000-request run
  was compared with a million-request baseline, and short runs spend a bigger share of their time warming up. The
  code was fine. Runs are now compared only with a baseline of the same size.

## What it doesn't do yet

- The editor has no formatting toolbar yet (keyboard shortcuts work); the full one from the playground comes next.
- No sign-out button in the Studio, and no search.
- "Keep mine" or "use theirs" is per article; merging the two paragraph by paragraph is later.
- Load numbers come from one laptop. They show the code holds up; they aren't a promise about production hardware.

## Up next

**1.4, the first simulator run:** thousands of randomized, repeatable runs of editors colliding, answers going missing
and browsers crashing, checked after every single step.
