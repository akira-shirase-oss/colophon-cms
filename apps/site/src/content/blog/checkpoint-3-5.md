+++
title = "Checkpoint 3.5: the Studio, designed"
description = "The Studio got a design of its own, twice: the first one looked like every other generated dashboard, so I rebuilt it from a reference I liked. Editors can now search titles, see what's live and what changed, compare versions and review exact diffs. And the load test that seemed to say the redesign made saves slower turned out to be measuring my laptop's ever-growing test database."
date = 2026-10-27T14:58:47Z

[extra]
kind = "release"
milestone = "P3"
step = "3.5"
short_title = "The Studio, designed"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 251

[extra.summary]
goal = "Give the Studio a design of its own and the editing pieces it was missing, without slowing anything down or breaking accessibility."
built = "A design system on every shadcn-svelte component with Colophon's own tokens, a page that shows all of it, title search, live and changed states on every entry, version comparison, review diffs, and a sign-in page to match."
proof = ["all text passes WCAG AA, and AAA in high contrast", "77 Studio tests in Chromium and 21 against the real server", "a million draft requests at 1,785 a second on a fresh database"]
code = "akira-shirase-oss/colophon-cms · main @ d1c9bd7"
+++

Step 3.4 left me with a CMS that works but looks like a developer's tool. Before my own site goes public on it, the
place where I write has to be somewhere I want to be.

## What I built

- **A design system, on everything.** All 64 shadcn-svelte components live in the repository as code I own, on one
  set of tokens: warm off-white paper, white sheets that float on soft shadows, near-black ink, black pill buttons,
  and one burnt-orange accent that only ever glows. Headings use Inter's display cut, set tight; numbers line up.
  Everything works in light, dark and high contrast.
- **A page that shows all of it.** `/studio/design` has every colour, type size, radius and shadow, and every
  component in the states the Studio uses, from buttons to calendars.
- **The editing pieces that were missing.** Search entries by title. See, on every entry, whether it's a draft,
  live, or changed since it went live. Compare versions. Review a publish request as an exact diff from
  what's live to what will be. The Studio got a sidebar, a command palette that searches titles, and an overview.
- **A sign-in page that looks like Colophon,** with the same paper, card and black pill.

## Does it actually work?

- **Contrast is a test, not a promise:** 124 checks read the stylesheet and hold every text colour to WCAG AA on
  every surface it's drawn on, and to AAA when the system asks for more contrast.
- **77 Studio tests in Chromium and 21 against the real server,** including accessibility checks on every page,
  and the 22 testbed tests through the real API.
- **A million draft requests** (reads, lists, title searches, version reads and saves) at 1,785 a second, 0 failed,
  the slowest 1% under 53 ms, on a fresh database. No regression against the baseline.
- **251 Rust tests,** and the simulator held every invariant over 2,000 seeds.

## Where I messed up

- **The first design looked generated.** It was competent and forgettable: everything uppercase, widely spaced and
  square. Most of that came from the component library's default preset. I rebuilt the look from a reference I
  actually liked, and swept the preset out of every component.
- **I reinstalled the components over my own fixes.** Refreshing all 64 from the library quietly undid an
  accessibility fix in the command palette, and ten Studio tests failed. Fixed, and I now compare every component's
  structure before overwriting anything.
- **I blamed the machine for the wrong reason.** Two million-request runs looked slower than the baseline. Part of
  it was the load tool itself: each version read had its own URL, so k6 kept a separate metric per request and grew
  to 1.7 GB. The rest was my local database, grown to 5 GB by weeks of test runs: saves were 3.5 times slower on it
  than on a fresh one. The full checks now always start from a fresh database, as CI does, and that growth is a real
  finding for production to fix.
- **One flaky test.** A testbed test timed out once in three runs while the machine was very busy; I haven't found
  why yet.

## What it doesn't do yet

- The Studio still speaks developer: "changeset", "destination", "probe". Plain words for editors come in step 3.7.
- Text fields have very light borders, like the reference; high-contrast mode darkens them, but by default they are
  below the 3:1 contrast WCAG asks of control outlines.
- My site still isn't on the internet.

## Up next

**Slugs and a content-type builder** (step 3.6): every entry gets a URL name of its own, and a type change is checked
against what's actually stored, naming the entries in its way. Then **plain words and one-click publishing** (3.7),
then my site goes live (3.8).

The bigger change: the goal is now every feature Strapi and Sanity offer, plus a proper role and permission system,
built to the same standard of proof. That's about 9 to 12 months of work, and the plan is in the roadmap.
