+++
title = "Checkpoint 0.6: the moment text becomes a web page"
description = "Colophon's renderer turns a stored article into the page readers see. There's one of it, it refuses rather than guesses, and a test reads back everything it produces to make sure nothing in it can run."
date = 2026-10-25T21:02:42Z

[extra]
kind = "experiment"
milestone = "P0"
step = "0.6"
short_title = "The moment text becomes a web page"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 81

[extra.summary]
goal = "Turn a stored article into a web page safely, and make the Studio's preview exactly what readers will see."
built = "The renderer, the expected page for every good test document, and a Preview tab in the Studio that uses the same code."
proof = ["12 of 12 pages match their reviewed output", "nothing produced can run code", "the preview is the same page, not a similar one"]
code = "akira-shirase-oss/colophon-cms · main @ c195f4a"
+++

Every CMS has a moment where text someone typed becomes a web page. That's the moment attackers aim for: if a
writer can slip a `<script>` into an article, it runs in every reader's browser.

This step builds that moment.

## What I built

- **The renderer,** which turns a stored article into the HTML of a web page.
- **The expected page for each of the 12 good test documents,** written out as files and reviewed by eye. They are now
  part of the shared test set, next to the documents themselves.
- **A Preview tab in the Studio** that shows the article as readers will see it.

## How it works (the short version)

**There's only one renderer.** The Studio's preview and the public site both call the same function. The usual approach
is two pieces of code that are *supposed* to agree; sooner or later they don't, and the preview starts lying.

**It builds every tag itself.** It never copies markup from the article. Text is escaped, so `<script>` in an article
comes out as the characters `<script>` on the page, harmless.

**It refuses rather than guesses.** Articles only reach it after the server has checked them, so anything unexpected is
a bug. Instead of trying to render something odd, it stops.

## Does it actually work?

- **All 12 good documents render to exactly the reviewed page.**
- **Every page it produces is read back** the way a browser would, and checked against a short list of allowed tags and
  attributes: no `onclick`, no `style`, no `<script>`, no link that runs code. All pass.
- **The classic tricks stay harmless:** a script typed into the text, a quote that tries to break out of an image
  description, text that tries to escape a code box, a quote hidden inside a link.
- **Breaking it on purpose:** I removed the escaping of quotes in attributes. Three tests failed straight away.
- **The Studio's preview is the same page as the reviewed one** for all 12 documents. Not similar: the same structure,
  checked by the browser itself.
- **62 renderer tests and 68 Studio tests in Chrome,** and CI passed all four jobs on the first run.

## The bug the preview found

I added the Preview tab and ran the Studio tests. Ten of them failed, and the page didn't just show an error: the whole
Studio stopped working.

Those ten were bad test documents the editor opens as they are (a heading level that doesn't exist, a link that runs
code) because the server is the one that judges them. The renderer correctly refused to draw them, and that refusal took
the page down with it.

On the public site, that strictness is right: the server stops bad articles long before they get there. But the Studio
shows drafts the server hasn't judged yet. So the Studio now shows "This document can't be previewed", with the reason,
and keeps working. The public site keeps the strict behaviour.

## Where I messed up

- **The renderer crashed on a document with no body,** with a generic error instead of a clear refusal. I'd checked that
  the pieces were right, but not that the pieces were there. It now checks the shape first.
- **Comparing pages as text didn't work.** The browser rewrites HTML as it reads it: `&#39;` becomes `'`, for example.
  The two pages were identical but the text wasn't. The test now compares what the browser built from each one.

## What it doesn't do yet

There's no public site yet; that arrives with the first demo in milestone P3. Images show a placeholder until media
uploads arrive.

## Up next

**0.7, Material 3 colours and the first screen:** the Studio gets a real design system, and the last step of the
foundation milestone is done.
