+++
title = "Checkpoint 0.5: the guard at the only door"
description = "The editor keeps honest writers honest. The server has to stop everyone else. Colophon's validator refuses all 34 bad test documents with exactly the right error and location, and checks 71,000 documents a second."
date = 2026-10-25T20:52:30Z

[extra]
kind = "experiment"
milestone = "P0"
step = "0.5"
short_title = "The guard at the only door"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 81

[extra.summary]
goal = "A server-side check that no bad article can get past, however it arrives."
built = "The Rust validator, an API endpoint that uses it, a testbed page that checks every test document through it, and load tests."
proof = ["34 of 34 bad documents refused, exactly", "12 of 12 good ones stored byte for byte like the editor", "1,000,000 validations at 71,270 a second, 0 failed"]
code = "akira-shirase-oss/colophon-cms · main @ 906a19e"
+++

The editor from the last step keeps honest writers from making mistakes. But the editor runs in *your* browser, and
anyone can skip it and send whatever they like straight to the server.

So the real rules have to live on the server, where every request passes through.

## What I built

- **The validator,** in Rust, in the part of Colophon that holds the rules. It takes an article in the stored format and
  either accepts it or refuses it with two things: a code saying what's wrong (`unsafe_link`, `too_deep`…) and the exact
  location in the document.
- **An API endpoint** that runs it, so the Studio, scripts and the testbed can ask "would this be accepted?" before
  saving.
- **A testbed page** that sends all 46 shared test documents through the real API and checks every verdict.
- **A load test** where every request is real work: read a whole article, check it, write the stored copy.

## How it works (the short version)

The validator walks through the document once, checking every rule in a fixed, written order, and writes the clean
stored copy as it goes. The first problem stops it, so the same bad document always gets the same answer.

Clean means *canonical*. Two documents that differ only in ways that don't matter, like the order of fields or extra
spaces, come out as exactly the same bytes. The editor produces the same bytes too, so a document saved from the editor
and one checked by the server are identical, not just similar.

## Does it actually work?

- **All 34 bad documents are refused with exactly the error and location they name.** Not just "refused": the right
  reason, at the right place, every time.
- **All 12 good documents are accepted,** and the stored copy matches the editor's byte for byte.
- **A document nested 100,000 levels deep** is refused at level 17, without crashing. Code that checks nesting by going
  all the way down would crash before it could answer.
- **1,000,000 validations** at 71,270 a second with 0 failures, and a steady 20,000 a second for a minute with 99% of
  answers in under 0.4 milliseconds. On GitHub's smaller test machine: 26,647 a second, 0 failures.
- **Breaking it on purpose:** with the link rule removed, two disguised links got through. With the unknown-field check
  removed, five documents with smuggled fields got through. The tests caught both.
- **81 Rust tests,** the testbed page agreeing on 46 of 46 documents in a real browser, and CI green on every job.

## The hole I found in my own rules

While writing the link rule, I realised a link like `/` + a tab + `/evil.example` would pass: it starts with one slash,
so it looks like a page on this site. But browsers quietly delete tabs from links, which turns it into
`//evil.example`: someone else's website. The same goes for a backslash, which browsers treat like a slash.

So the format now refuses tabs, spaces, line breaks and backslashes in links outright, in the editor and on the server,
and two new test documents make sure it stays that way.

## Where I messed up

- **A test document that tested nothing.** One bad document was supposed to contain an ordered list starting at 0. The
  little script that generates the test files treated 0 as "nothing", so the file had no starting number at all: a
  perfectly valid list. The validator was right to accept it. Only the exact-answer check noticed; the editor's tests
  couldn't. One line fixed, and it is now a test document that actually tests something.
- **A crash in the test, not the code.** To build the 100,000-level document, I used a shortcut that quietly copies the
  whole document again on every level. The test crashed before the validator even ran. Building it by hand fixed it.
- **Rules I changed my mind on.** I first planned to refuse documents whose formatting marks came in an unusual order.
  But order means nothing there: bold-then-italic and italic-then-bold look the same. Now the validator puts them in the
  standard order instead.

## What it doesn't do yet

Nothing is stored yet: saving articles is the next milestone. The endpoint is open to anyone for now, and will need a
login along with the rest of the API.

## Up next

**0.6, the renderer:** turning a stored article into a web page, the same way in the Studio preview and on the public
site, and never producing anything that can run code.
