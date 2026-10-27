+++
title = "Checkpoint 3.4: a site built from one release"
description = "Colophon now runs a real website: every release is built into a static site where every page says which release it is, checked, and swapped in all at once, so a reader never gets half a release. A probe then reads the live page, and the release's receipt says the site really shows it. It runs on my laptop for now: before it goes public, the Studio gets designed properly."
date = 2026-10-26T19:35:21Z

[extra]
kind = "release"
milestone = "P3"
step = "3.4"
short_title = "A site built from one release"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 249

[extra.summary]
goal = "Run a real static site from Colophon's releases, prove which release every page is, and never show a reader a half-built or older site."
built = "A small site builder that reads one release, marks every page, verifies the build and promotes it atomically; a runner that rebuilds on each release; a one-command local demo."
proof = ["publish in the Studio, see it verified on the live site", "an older build can never replace a newer one", "a million page reads during 27 releases, every page whole"]
code = "akira-shirase-oss/colophon-cms · main @ 0864e22"
+++

A fresh answer from Colophon's API proves nothing about a static website: the site might not have been rebuilt yet,
or a rebuild might have failed halfway. So my own site is the hard case, and this step builds it.

## What I built

- **A site built from exactly one release.** The builder asks Colophon's public API for release 12, and every
  request it makes names release 12, so a release published while it builds can't sneak half its changes in. It
  renders articles with the same code the Studio's preview uses.
- **Every page says which release it is.** Each page carries a small tag with the release number, exactly once.
  That's what a probe reads.
- **Checked, then swapped in all at once.** A build goes into a folder of its own. Before it goes live, every page is
  checked for the right release tag, and then the live site switches to it with one rename: a reader gets the old
  site or the new one, never a mix.
- **Never backwards.** If a slow build of release 11 finishes after release 12 is live, it's refused. A failed build
  leaves the live site alone.
- **A runner** that hears about each release, builds the newest one, and serves the site.
- **A one-command local demo** (`scripts/demo.sh`): the CMS, the Studio and the site, on a database of their own,
  with a sample project wired up.

## Does it actually work?

- **End to end, in a browser:** an article is approved and published in the Studio; the site rebuilds; the probe
  reads the live page; the release's receipt says *Verified at 1 location*; and the live pages pass the
  accessibility checks.
- **The whole path, against the real server:** three releases, including one that takes an article down. Each
  is verified on the rebuilt site, and the article taken down is gone from it.
- **A million page reads while releases kept coming:** 1,000,195 reads at 19,013 a second, the slowest 1% under 7
  ms, while 27 releases were published, one every two seconds, each built and swapped in underneath the readers.
  Every page read was whole, with exactly one release tag, and the site ended on release 27 of 27.
- **249 Rust tests,** 21 Studio tests against the server, 21 testbed tests, and CI passed every job
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40385776000)).

## Where I messed up

- **I built the branch in the wrong place.** I created this step's branch inside the previous step's folder, where
  a load test was about to write its results. Nothing had changed yet, so I moved it to a folder of its own.
- **The runner needed to know its project before it existed.** The load test creates its project after the runner
  starts, so the runner now adopts the project of the first release it hears about, and refuses any other.
- **The demo showed its age.** Walking through it in a browser, the Studio's header still said "step 1.7" from weeks
  of steps ago, and a receipt printed "HTTP 503 (HTTP 503)". Both fixed. The bigger finding was that the Studio
  works but looks like a developer's tool, which is why the next step is design.

## What it doesn't do yet

- It isn't on the internet. My site goes live on Colophon after the Studio is designed and I've tried the whole CMS
  myself.
- No images, feeds or sitemaps on the site yet.
- The `/colophon` page names the release, but not its receipt: receipts need a sign-in, and a page can't contain the
  check of itself.

## Up next

**Designing the Studio:** a design system of its own on Tailwind and shadcn, informed by Strapi, Sanity and Convex
running side by side, and the missing pieces of the editing experience.
