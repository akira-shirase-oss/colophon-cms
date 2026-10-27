+++
title = "Checkpoint 0.7: colours that pass the test, and the end of the foundation"
description = "Colophon's design system: Material 3 colours generated from one blue, checked for contrast in every theme by CI, and an accessibility scanner that found two real problems on its first run. With this, the foundation milestone is done."
date = 2026-10-25T21:32:26Z

[extra]
kind = "release"
milestone = "P0"
step = "0.7"
short_title = "Colours that pass the test"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 81

[extra.summary]
goal = "A real design system for the Studio, readable by everyone, in light, dark and high contrast."
built = "Material 3 colours from one brand colour, the components the Studio needs, and the Studio's first screen rebuilt on them."
proof = ["116 contrast checks pass in 6 colour schemes", "0 accessibility violations in 4 themes", "milestone P0 done"]
code = "akira-shirase-oss/colophon-cms · main @ 1455073"
+++

Light grey text on a white background looks elegant in a design tool. Then someone opens it on a phone in the sun, or
with tired eyes, and can't read a word.

This step gives the Studio a design system, and makes "readable" something CI checks rather than something I hope.

## What I built

- **Colours from one blue.** Google's Material 3 design system can generate a whole colour scheme from a single brand
  colour: buttons, backgrounds, text, borders, warnings, all chosen to work together. Colophon generates six schemes
  from its blue: light and dark, each at normal, medium and high contrast.
- **The pieces the Studio needs:** a toolbar, tabs, a dialog, a menu, buttons and text fields, built on Bits UI, a
  library that handles the hard part (keyboard use and screen readers) and leaves the look to me.
- **The Studio's first real screen,** rebuilt on all of it, with an Appearance menu for light, dark and contrast.
- **A self-hosted font,** so the Studio never asks anyone else's server for anything.

## How it works (the short version)

The colours are generated once, by a script, and saved in a file. A test regenerates them and fails if the file is out
of date, so nobody can tweak a colour by hand and forget to check it.

Every colour that has text drawn on it is checked against that text using the official accessibility formula. Normal
schemes need a contrast ratio of at least 4.5 to 1, the WCAG "AA" level. High contrast needs 7 to 1, the stricter "AAA"
level.

## Does it actually work?

- **116 contrast checks pass** across all six schemes. The closest call is 4.53 to 1, just above the 4.5 line. Close,
  but a pass, and now I know where the edge is.
- **An automated accessibility scanner** (axe) checks the editor, the preview and the image dialog in light, dark and
  both high-contrast themes. Zero problems.
- **The keyboard works the way it should:** the formatting toolbar is one Tab stop and the arrow keys move inside it; the
  dialog keeps focus inside, closes on Escape, and puts you back where you were.
- **Your choice of theme survives a reload,** and the Studio makes no requests to any other website.
- **76 Studio tests in a real browser,** plus everything from before, and CI passed all four jobs
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40236809898)).

## What the scanner found

The accessibility scanner's first run failed in every theme, and both problems were real:

- **The writing area had no name.** A screen reader user would land on it and hear "text box", with no idea what it was
  for. It's now announced as "Article body".
- **The stored-JSON panel scrolls, but couldn't be reached with a keyboard.** If you can't use a mouse, you couldn't read
  past the first screen of it. It's focusable now.

Neither was visible on screen, which is exactly why the scanner exists.

## Where I messed up

- **The first colour scheme dulled the brand.** Material's default scheme turned the bright blue into a greyish blue. A
  different scheme keeps the colour exactly as chosen.
- **The green was too loud.** The "success" colour came out as neon mint next to the blue. I calmed it, and the contrast
  checks confirmed it's still readable.
- **A test that expected the wrong thing.** I assumed switching to high contrast changes the background. It doesn't:
  Material keeps backgrounds the same and makes text and borders stronger. The test now checks a colour that does
  change.
- **The colour library's newest version doesn't load in Node at all,** because of a missing file extension in one
  import. I'm using the previous version and wrote down why.

## The foundation is done

That was the last step of milestone P0: pinned tools, a local stack that proves itself, honest health checks, the
testing rule, the editor, the server-side check, the renderer and now the design system. Every question the plan left
open for this milestone has a written answer.

## Up next

**Milestone P1, never lose an edit:** signing in, projects that can't see each other, and drafts that survive anything.
