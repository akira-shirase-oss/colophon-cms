+++
title = "Checkpoint 0.4: an editor that would rather refuse than lose a word"
description = "Colophon's writing editor and the format behind it. The editor opens a document only if it can give it back byte for byte, because the library underneath quietly throws away whatever it doesn't understand."
date = 2026-10-25T20:31:38Z

[extra]
kind = "experiment"
milestone = "P0"
step = "0.4"
short_title = "An editor that would rather refuse than lose a word"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 63

[extra.summary]
goal = "A writing editor whose output Colophon can trust: what goes in comes back out, exactly."
built = "The rich-text format, 42 shared test documents (12 good, 30 bad), the editor, and the first screen of the Studio."
proof = ["12 of 12 documents come back byte for byte", "unknown content is refused, never trimmed", "104 frontend tests, CI green on the first run"]
code = "akira-shirase-oss/colophon-cms · main @ eb742b4"
+++

Copy a paragraph from a website and paste it somewhere. You bring along things you can't see: hidden styles, tracking
pixels, sometimes a script.

A CMS editor has to keep the words and throw the rest away. And it must never throw away *your* words.

## What I built

- **The format.** A short, written list of everything an article may contain: paragraphs, headings, lists, quotes, code,
  images, a "callout" box for notes and warnings, and bold, italic, links and so on. No HTML, no scripts, nothing that
  runs. Images can only come from Colophon's own media library, and they need a text description for people who can't
  see them.
- **42 test documents** shared by every part of Colophon that touches articles. 12 are good. 30 are bad on purpose: a
  script hidden in a node, a link that runs code, an image pointing at another site, a document nested 17 levels deep.
  Each bad one says exactly which error it must produce, and where.
- **The editor,** built on Tiptap, the editing library a lot of modern apps use, configured to allow exactly what the
  format allows and nothing more.
- **The Studio's first screen:** the editor with a toolbar, the stored result shown live next to it, and a menu to open
  any of the 42 test documents.

## How it works (the short version)

Tiptap is built on ProseMirror. When ProseMirror loads a document containing something it doesn't recognise, it quietly
drops it. No error, no warning. Open, save, and part of the document is gone.

Tiptap can check for unknown *kinds* of content. It doesn't check for unknown *details*, like an extra field on a
paragraph. So Colophon adds one more step: after opening a document, it converts it straight back and compares the
result with what was stored. If a single byte differs, the document isn't opened, and the screen says exactly where the
difference was.

Lossless, or refused. Never trimmed.

## Does it actually work?

- **All 12 good documents** open and come back byte for byte, twice in a row, in a test environment and in real Chrome.
  That includes emoji, right-to-left text, Chinese and Korean, and code with its exact spacing.
- **Of the 30 bad ones,** the editor refuses 20. The other 10 it keeps exactly as they are, so nothing is lost. Those
  10 break rules about values, like a heading level that doesn't exist or a link that runs code, and the editor isn't
  the judge of those. The server is, and that's the next step.
- **Without the compare step, 7 of the bad documents would have been quietly changed on save.** I removed it on purpose
  to check: 7 tests failed.
- **Pasting** a page full of scripts, styles, an embedded frame, a tracking pixel and a `javascript:` link keeps the
  words, the bold and the safe link, and nothing else.
- **The editor can't create** what the format forbids: no unsafe links, no level-1 headings (that's the article's title),
  no image without a description, no nesting past the limit.
- **104 frontend tests** (54 fast ones, 50 in real Chrome), plus everything from before. The first CI run passed all
  four jobs ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40230822617)).

## Where I messed up

- **I wrote down the wrong order** for formatting marks in the format document. Then I checked it against the real
  editor, which keeps them in a different order. The document now matches what the code actually does.
- **A false comment.** I switched off a Tiptap feature that adds an empty paragraph at the end of a document, and wrote
  that it changed documents "just by being opened". When I turned it back on, every test still passed, so the comment
  was wrong. The real problem: it adds the paragraph after your *first edit*, so fixing one typo would also change the
  end of the article. There's now a test for exactly that, and it fails when the feature is on.
- **An undo test that misunderstood undo.** I typed two things and expected one undo to remove only the second. But the
  editor groups anything typed within half a second into one undo step, the way every text editor does. The test now
  pauses like a person would.
- **A paste test that tested Chrome instead of my code.** When you copy HTML in Chrome, Chrome cleans some of it itself
  before the editor ever sees it. So the browser test couldn't show whether *my* filtering works. A second test feeds the
  raw, uncleaned HTML straight into the editor.

## What it doesn't do yet

Nothing is saved anywhere yet, and the server doesn't check documents. Images are placeholders until media uploads arrive
in a later milestone.

## Up next

**0.5, Rust refuses what the editor must never store:** the server-side check. All 30 bad documents must be refused,
each with its exact error and location, and all 12 good ones accepted.
